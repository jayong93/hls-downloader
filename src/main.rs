use anyhow::anyhow;
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use indicatif::*;
use lazy_static::*;
use m3u8_rs::{playlist::*, *};
use std::cell::UnsafeCell;
use std::cmp::{Eq, Ord, Ordering, PartialEq, PartialOrd};
use std::collections::BinaryHeap;
use url::*;

lazy_static! {
    pub static ref REQ_CLIENT: reqwest::Client = reqwest::Client::new();
    static ref TIME_PATTERN: regex::Regex = regex::Regex::new(
        r"^\s*(?:(?:(?:(\d+):)?(?:(\d+):))?(\d+)-)?(?:(?:(?:(\d+):)?(?:(\d+):))?(\d+))?\s*$"
    )
    .unwrap();
}
const MAX_FUTURE_NUM: usize = 10;

#[derive(Clone, Debug)]
struct HLSVideo {
    url: Url,
    name: String,
    range: (f32, f32),
}

#[tokio::main]
async fn main() {
    let args = get_args(clap::App::new("Twitch Downloader"));
    let urls = get_hls_videos(&args);
    let hls_list = get_hls_playlist(urls, &args).await;
    gstreamer::init().unwrap();
    download_video(hls_list, &args).await.await.unwrap();
}

async fn master_to_media(
    org_url: &Url,
    master_list: m3u8_rs::playlist::MasterPlaylist,
) -> (Url, m3u8_rs::playlist::MediaPlaylist) {
    let target_url = &master_list.variants.first().unwrap().uri;
    let media_url = Url::parse(target_url)
        .and_then(|u| {
            if !u.cannot_be_a_base() {
                Ok(u)
            } else {
                org_url.join(target_url)
            }
        })
        .or(org_url.join(target_url))
        .unwrap();
    (
        media_url.clone(),
        REQ_CLIENT
            .get(media_url)
            .send()
            .map_err(|e| eprintln!("Error: {}", e))
            .and_then(|res| res.bytes().map_err(|e| eprintln!("Error: {}", e)))
            .map_ok(|b| parse_media_playlist(b.as_ref()).unwrap().1)
            .map(|res| res.unwrap())
            .await,
    )
}

async fn get_hls_playlist(
    videos: Vec<HLSVideo>,
    args: &clap::ArgMatches<'_>,
) -> Vec<Result<(HLSVideo, MediaPlaylist), reqwest::Error>> {
    stream::iter(videos)
        .map(|mut hls_video| async {
            let bytes = REQ_CLIENT
                .get(hls_video.url.clone())
                .send()
                .and_then(|res| res.bytes())
                .await;
            match bytes {
                Ok(bytes) => {
                    let parsed_playlist = parse_playlist_res(bytes.as_ref()).unwrap();
                    if args.is_present("print-debug") {
                        let contents = std::str::from_utf8(bytes.as_ref()).unwrap();
                        eprintln!("DEBUG: for url '{}'\n original playlist -> {}\n parsed playlist -> {:#?}", &hls_video.url, contents, &parsed_playlist);
                    }
                    match parsed_playlist {
                    Playlist::MasterPlaylist(master_list) => {
                        let (new_url, media_list) =
                            master_to_media(&hls_video.url, master_list).await;
                        hls_video.url = new_url;
                        Ok((hls_video, media_list))
                    }
                    Playlist::MediaPlaylist(media_list) => Ok((hls_video, media_list)),
                }},
                Err(e) => Err(e),
            }
        })
        .buffer_unordered(MAX_FUTURE_NUM)
        .collect::<Vec<_>>()
        .await
}

fn get_args<'a>(app: clap::App<'a, '_>) -> clap::ArgMatches<'a> {
    app.version(env!("CARGO_PKG_VERSION"))
        .author("SyuJyo <jayong93@gmail.com>")
        .about("Download any hls playlist")
        .arg(
            clap::Arg::with_name("VIDEO_DATA")
                .help("HLS Video data to be downloaded. You should provide a video url and the name. And you can give a optional play range. The url, name and play range should be delimited by semicolon. The format of play range is: [START_TIME-][END_TIME]. Each time has same format: [[HOUR:]MIN:]SEC.")
                .multiple(true)
                .required(true)
                .empty_values(false)
                .index(1)
                .takes_value(true)
                .number_of_values(1)
                .validator(|s| str_to_hls_video(&s).map(|_| ()))
        )
        .arg(
            clap::Arg::with_name("out_dir")
                .help("Where temporary videos and full videos will be saved in.")
                .long("out-dir")
                .short("o")
                .empty_values(false)
                .value_name("OUT_DIR")
                .default_value("."),
        )
        .arg(
            clap::Arg::with_name("print-debug")
                .help("Print informations for debugging")
                .long("debug")
                .takes_value(false)
        )
        .get_matches()
}

fn str_to_hls_video(s: &str) -> Result<HLSVideo, String> {
    let mut it = s.split(';');
    let url = it
        .next()
        .ok_or("You should provide url of a video".to_owned())
        .and_then(|u| Url::parse(u.trim()).map_err(|e| format!("{}", e)))?;
    let name = it
        .next()
        .and_then(|n| {
            let n = n.trim();
            if n.len() == 0 {
                None
            } else {
                Some(n)
            }
        })
        .ok_or("You should provide name of a video".to_owned())?
        .to_string();
    let range = if let Some(time_data) = it.next() {
        let caps = TIME_PATTERN
            .captures(time_data.trim())
            .ok_or("Wrong play range format. The format is: [START_TIME-][END_TIME]. Each time has same format: [[HOUR:]MIN:]SEC")?;
        let times: Vec<usize> = caps
            .iter()
            .skip(1)
            .map(|m| m.and_then(|mat| mat.as_str().parse().ok()).unwrap_or(0))
            .collect();
        (
            (times[0] * 3600 + times[1] * 60 + times[2]) as f32,
            (times[3] * 3600 + times[4] * 60 + times[5]) as f32,
        )
    } else {
        (0f32, 0f32)
    };

    Ok(HLSVideo { url, name, range })
}

fn get_hls_videos(args: &clap::ArgMatches<'_>) -> Vec<HLSVideo> {
    args.values_of("VIDEO_DATA")
        .map(|values| values.map(|data| str_to_hls_video(data).unwrap()).collect())
        .unwrap()
}

struct NonCopyable<T>(T);

async fn download_video(
    video_list: Vec<Result<(HLSVideo, m3u8_rs::playlist::MediaPlaylist), reqwest::Error>>,
    args: &clap::ArgMatches<'_>,
) -> oneshot::Receiver<()> {
    let out_dir = std::path::Path::new(args.value_of("out_dir").unwrap()).to_path_buf();
    std::fs::create_dir_all(&out_dir).expect("Can't create an output directory.");

    let multi_pb = MultiProgress::new();

    for video in video_list.into_iter() {
        match video {
            Ok((hls_video, playlist)) => {
                let (start_time, end_time) = hls_video.range;
                let name = &hls_video.name;
                let cumul_time = UnsafeCell::new(0f32);
                let mut total_bytes: Option<i32> = None;
                let content_length_list: Vec<_> = playlist
                    .segments
                    .into_iter()
                    .skip_while(|chunk| {
                        if start_time == 0.0 {
                            return false;
                        }
                        let cumul_time = unsafe { &mut *cumul_time.get() };
                        let chunk_end_time = *cumul_time + chunk.duration;
                        if chunk_end_time < start_time {
                            *cumul_time = chunk_end_time;
                            true
                        } else {
                            false
                        }
                    })
                    .take_while(|chunk| {
                        if end_time == 0.0 {
                            return true;
                        }
                        let chunk_start_time = unsafe { &mut *cumul_time.get() };
                        if *chunk_start_time <= end_time {
                            *chunk_start_time += chunk.duration;
                            true
                        } else {
                            false
                        }
                    })
                    .inspect(|chunk| match (total_bytes, chunk.byte_range.as_ref()) {
                        (Some(total), Some(byte)) => {
                            total_bytes = Some(total + byte.length);
                        }
                        (None, Some(byte)) => {
                            total_bytes = Some(byte.length);
                        }
                        _ => {
                            total_bytes = None;
                        }
                    })
                    .enumerate()
                    .collect();

                let pb = ProgressBar::hidden();
                if let Some(total) = total_bytes {
                    pb.set_length(total as u64);
                } else {
                    pb.set_length(0);
                }
                pb.set_style(ProgressStyle::default_bar().template(
                    "{prefix:12}. [{elapsed_precise}] {bytes}/{total_bytes} {bytes_per_sec}",
                ));
                pb.enable_steady_tick(1000);
                let pb = multi_pb.add(pb);

                let merge_pb = ProgressBar::hidden();
                merge_pb.set_length(content_length_list.len() as _);
                merge_pb.set_style(
                    ProgressStyle::default_bar().template("{prefix:12}. {pos}/{len} chunks merged"),
                );
                merge_pb.enable_steady_tick(1000);
                let merge_pb = multi_pb.add(merge_pb);

                let mut out_path = out_dir.clone();
                out_path.push(name.to_string() + ".ts");

                if let Some(files) = out_path
                    .parent()
                    .and_then(|out_dir| out_dir.read_dir().ok())
                    .map(|dir_it| {
                        dir_it
                            .filter_map(|item| item.ok().map(|entry| entry.file_name()))
                            .collect::<Vec<_>>()
                    })
                {
                    let file_name = out_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_owned();
                    let file_ext = out_path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_owned();
                    let pattern = format!(r"{}(?:\((\d+)\))?\.{}", file_name, file_ext);
                    let pattern = regex::Regex::new(&pattern).unwrap();
                    let max_num: Option<u32> = files
                        .iter()
                        .filter_map(|entry| pattern.captures(entry.to_str().unwrap()))
                        .filter_map(|cap| {
                            cap.get(1).map(|m| m.as_str()).unwrap_or("0").parse().ok()
                        })
                        .max();
                    if let Some(max_num) = max_num {
                        out_path.set_file_name(format!(
                            "{}({}).{}",
                            file_name,
                            max_num + 1,
                            file_ext
                        ));
                    }
                }

                pb.set_prefix(out_path.as_os_str().to_str().unwrap());
                merge_pb.set_prefix(out_path.as_os_str().to_str().unwrap());

                if let Some(files) = out_path
                    .parent()
                    .and_then(|out_dir| out_dir.read_dir().ok())
                    .map(|dir_it| {
                        dir_it
                            .filter_map(|item| item.ok().map(|entry| entry.file_name()))
                            .collect::<Vec<_>>()
                    })
                {
                    let file_name = out_path
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_owned();
                    let file_ext = out_path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("")
                        .to_owned();
                    let pattern = format!(r"{}(?:\((\d+)\))?\.{}", file_name, file_ext);
                    let pattern = regex::Regex::new(&pattern).unwrap();
                    let max_num: Option<u32> = files
                        .iter()
                        .filter_map(|entry| pattern.captures(entry.to_str().unwrap()))
                        .inspect(|cap| eprintln!("{:?}", cap))
                        .filter_map(|cap| {
                            cap.get(1).map(|m| m.as_str()).unwrap_or("0").parse().ok()
                        })
                        .max();
                    if let Some(max_num) = max_num {
                        out_path.set_file_name(format!(
                            "{}({}).{}",
                            file_name,
                            max_num + 1,
                            file_ext
                        ));
                    }
                }

                pb.set_prefix(out_path.as_os_str().to_str().unwrap());
                merge_pb.set_prefix(out_path.as_os_str().to_str().unwrap());

                tokio::spawn(async move {
                    let (comp_send, comp_recv) = oneshot::channel();
                    {
                        let sender = data_send(out_path, comp_send, merge_pb).await;
                        let pb = pb.clone();
                        stream::iter(content_length_list)
                            .map(|(idx, chunk)| {
                                let idx_move = NonCopyable(idx); // Copy 때문에 일어나는 referencing을 제거하기 위한 꼼수
                                async {
                                    let chunk = chunk;
                                    let idx = idx_move;
                                    let res = REQ_CLIENT
                                        .get(hls_video.url.join(&chunk.uri).unwrap())
                                        .send()
                                        .map_err(|e| eprintln!("{}", anyhow!(e)))
                                        .await?;

                                    pb.inc_length(res.content_length().unwrap_or(0));
                                    let bytes = res
                                        .bytes()
                                        .map_err(|e| eprintln! {"{}", anyhow!(e)})
                                        .await?;
                                    pb.inc(bytes.len() as u64);
                                    sender
                                        .unbounded_send((idx.0, bytes))
                                        .map_err(|e| eprintln!("{}", anyhow!(e)))
                                }
                            })
                            .buffer_unordered(MAX_FUTURE_NUM)
                            .for_each(|_| future::ready(()))
                            .await;
                    }
                    pb.set_length(pb.position());
                    comp_recv.await.ok();
                    pb.finish();
                });
            }
            Err(e) => {
                eprintln!("Couldn't get video properly: {}", e);
            }
        }
    }

    let (sender, receiver) = oneshot::channel();

    std::thread::spawn(move || {
        multi_pb.join().unwrap();
        sender.send(()).ok();
    });

    receiver
}

#[derive(Eq, PartialEq)]
struct IndexedByte {
    idx: usize,
    data: Bytes,
}

impl PartialOrd for IndexedByte {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for IndexedByte {
    fn cmp(&self, other: &Self) -> Ordering {
        self.idx.cmp(&other.idx).reverse()
    }
}

async fn data_send(
    out_file: std::path::PathBuf,
    completion_sender: oneshot::Sender<()>,
    pb: ProgressBar,
) -> mpsc::UnboundedSender<(usize, Bytes)> {
    use gst::prelude::*;
    use gstreamer as gst;
    use gstreamer_app as gst_app;

    let (sender, mut receiver) = mpsc::unbounded();

    tokio::spawn(async move {
        let concat = gst::ElementFactory::make("concat", Some("c")).unwrap();
        let fsink = gst::ElementFactory::make("filesink", None).unwrap();
        fsink
            .set_property("location", &out_file.to_str().unwrap())
            .unwrap();
        let appsrc = gst::ElementFactory::make("appsrc", None).unwrap();

        let pipeline = gst::Pipeline::new(None);
        pipeline.add_many(&[&concat, &fsink, &appsrc]).unwrap();
        gst::Element::link_many(&[&concat, &fsink]).unwrap();
        gst::Element::link_many(&[&appsrc, &concat]).unwrap();

        let appsrc = appsrc.dynamic_cast::<gst_app::AppSrc>().unwrap();

        pipeline.set_state(gst::State::Playing).unwrap();

        let mut cur_idx = 0usize;
        let mut heap = BinaryHeap::new();
        while let Some((idx, b)) = receiver.next().await {
            if idx == cur_idx {
                appsrc
                    .push_buffer(gst::buffer::Buffer::from_slice(b))
                    .unwrap();
                cur_idx += 1;
                pb.inc(1);
            } else {
                heap.push(IndexedByte { idx, data: b });
                match heap.peek() {
                    Some(val) if val.idx == cur_idx => {
                        appsrc
                            .push_buffer(gst::buffer::Buffer::from_slice(heap.pop().unwrap().data))
                            .unwrap();
                        cur_idx += 1;
                        pb.inc(1);
                    }
                    _ => {}
                }
            }
        }
        while !heap.is_empty() {
            appsrc
                .push_buffer(gst::buffer::Buffer::from_slice(heap.pop().unwrap().data))
                .unwrap();
            pb.inc(1);
        }
        appsrc.end_of_stream().unwrap();
        pb.finish();

        let bus = pipeline.get_bus().unwrap();

        use gst::MessageView;
        for msg in bus.iter_timed(gst::CLOCK_TIME_NONE) {
            match msg.view() {
                MessageView::Error(e) => {
                    eprintln!("{}", e.get_error());
                    break;
                }
                MessageView::Eos(_) => {
                    break;
                }
                _ => (),
            }
        }

        pipeline.set_state(gst::State::Null).unwrap();
        completion_sender.send(()).ok();
    });

    sender
}
