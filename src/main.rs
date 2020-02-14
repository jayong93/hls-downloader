use anyhow::anyhow;
use bytes::Bytes;
use futures::channel::{mpsc, oneshot};
use futures::prelude::*;
use indicatif::*;
use lazy_static::*;
use m3u8_rs::{playlist::*, *};
use std::cell::UnsafeCell;
use url::*;

lazy_static! {
    pub static ref REQ_CLIENT: reqwest::Client = reqwest::Client::new();
    static ref TIME_PATTERN: regex::Regex = regex::Regex::new(
        r"^\s*(?:(?:(?:(\d+):)?(?:(\d+):))?(\d+)-)?(?:(?:(?:(\d+):)?(?:(\d+):))?(\d+))?\s*$"
    )
    .unwrap();
}
const MAX_FUTURE_NUM: usize = 500;

#[tokio::main]
async fn main() {
    let args = get_args(clap::App::new("Twitch Downloader"));
    let urls = get_urls(&args);
    let hls_list = get_hls_data(urls).await;
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

async fn get_hls_data(urls: Vec<Url>) -> Vec<Option<(Url, MediaPlaylist)>> {
    stream::iter(urls)
        .map(|url| async {
            let bytes = REQ_CLIENT
                .get(url.clone())
                .send()
                .map_err(|e| eprintln!("Error: {}", e))
                .and_then(|res| res.bytes().map_err(|e| eprintln!("Error: {}", e)))
                .await;
            if let Some(bytes) = bytes.ok() {
                match parse_playlist_res(bytes.as_ref()).unwrap() {
                    Playlist::MasterPlaylist(master_list) => {
                        let (new_url, media_list) = master_to_media(&url, master_list).await;
                        Some((new_url, media_list))
                    }
                    Playlist::MediaPlaylist(media_list) => Some((url, media_list)),
                }
            } else {
                None
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
            clap::Arg::with_name("URL")
                .help("HLS URLs to be downloaded. If neither an input file nor an url is provided, it reads urls from stdin.")
                .multiple(true)
                .required(true)
                .empty_values(false)
                .index(1)
                .takes_value(true)
                .number_of_values(1)
        )
        .arg(
            clap::Arg::with_name("out_name")
                .multiple(true)
                .required(true)
                .short("o")
                .empty_values(false)
                .takes_value(true)
                .number_of_values(1)
                .value_name("OUT_NAME")
        )
        .arg(
            clap::Arg::with_name("out_dir")
                .help("Where temporary videos and full videos will be saved in.")
                .long("out-dir")
                .empty_values(false)
                .value_name("OUT_DIR")
                .default_value("."),
        )
        .arg(
            clap::Arg::with_name("duration")
                .help("Cut replay with specified duration")
                .long_help(
"Cut replay with specified duration.
The format is: [START_TIME-][END_TIME]
Each time has same format: [[HOUR:]MIN:]SEC"
                )
                .short("d")
                .long("duration")
                .value_name("DURATION")
                .multiple(true)
                .takes_value(true)
                .number_of_values(1)
                .validator(|s| if TIME_PATTERN.is_match(&s) {Ok(())} else {Err("Wrong duration format. please read help message with '--help'".to_owned())})
        )
        .get_matches()
}

fn get_urls(args: &clap::ArgMatches<'_>) -> Vec<Url> {
    args.values_of("URL")
        .map(|url_it| url_it.map(|s| Url::parse(s).unwrap()).collect())
        .unwrap()
}

async fn download_video(
    video_list: Vec<Option<(Url, m3u8_rs::playlist::MediaPlaylist)>>,
    args: &clap::ArgMatches<'_>,
) -> oneshot::Receiver<()> {
    let out_dir = std::path::Path::new(args.value_of("out_dir").unwrap()).to_path_buf();
    let out_names = args.values_of("out_name").unwrap().collect::<Vec<_>>();
    std::fs::create_dir_all(&out_dir).expect("Can't create an output directory.");

    let multi_pb = MultiProgress::new();

    let times: Vec<_> = if let Some(time_it) = args.values_of("duration") {
        time_it
            .map(|time| {
                let caps = TIME_PATTERN.captures(time).unwrap();
                let times: Vec<usize> = caps
                    .iter()
                    .skip(1)
                    .map(|m| m.and_then(|mat| mat.as_str().parse().ok()).unwrap_or(0))
                    .collect();
                (
                    (times[0] * 3600 + times[1] * 60 + times[2]) as f32,
                    (times[3] * 3600 + times[4] * 60 + times[5]) as f32,
                )
            })
            .inspect(|(start_time, end_time)| {
                if *end_time != 0.0 {
                    assert!(
                        start_time <= end_time,
                        "Start time should be smaller than end time"
                    );
                }
            })
            .collect()
    } else {
        vec![]
    };

    let max_future_per_video = MAX_FUTURE_NUM / video_list.len();

    for (idx, video) in video_list.into_iter().enumerate() {
        let (start_time, end_time) = times.get(idx).copied().unwrap_or((0f32, 0f32));

        match (video, out_names.get(idx)) {
            (Some((url, playlist)), Some(name)) => {
                let cumul_time = UnsafeCell::new(0f32);
                let content_length_list: Vec<_> = stream::iter(playlist.segments)
                    .skip_while(|chunk| {
                        if start_time == 0.0 {
                            return future::ready(false);
                        }
                        let cumul_time = unsafe { &mut *cumul_time.get() };
                        let chunk_end_time = *cumul_time + chunk.duration;
                        if chunk_end_time < start_time {
                            *cumul_time = chunk_end_time;
                            future::ready(true)
                        } else {
                            future::ready(false)
                        }
                    })
                    .take_while(|chunk| {
                        if end_time == 0.0 {
                            return future::ready(true);
                        }
                        let chunk_start_time = unsafe { &mut *cumul_time.get() };
                        if *chunk_start_time <= end_time {
                            *chunk_start_time += chunk.duration;
                            future::ready(true)
                        } else {
                            future::ready(false)
                        }
                    })
                    .map(|chunk| {
                        REQ_CLIENT
                            .head(url.join(&chunk.uri).unwrap())
                            .send()
                            .map_ok(|res| res.content_length().unwrap_or(0))
                            .map(|res| (res.unwrap_or(0), chunk))
                    })
                    .buffered(20)
                    .collect()
                    .await;

                let pb = ProgressBar::hidden();
                pb.set_style(
                    ProgressStyle::default_bar()
                        .template("{prefix:12}. [{elapsed_precise}] {wide_bar} {bytes}/{total_bytes} {bytes_per_sec}"),
                );
                pb.enable_steady_tick(1000);
                pb.set_length(content_length_list.iter().map(|(len, _)| len).sum());
                let pb = multi_pb.add(pb);

                pb.set_prefix(&name);

                let mut out_path = out_dir.clone();
                out_path.push(name.to_string() + ".mp4");

                tokio::spawn(async move {
                    let sender = data_send(out_path).await;
                    {
                        let pb = pb.clone();
                        stream::iter(content_length_list)
                            .map(|(_, chunk)| chunk)
                            .then(|chunk| REQ_CLIENT.get(url.join(&chunk.uri).unwrap()).send())
                            .map_err(|e| eprintln!("{}", anyhow!(e)))
                            .map_ok(|res| {
                                stream::unfold(Some(res), |res| async {
                                    if let Some(mut res) = res {
                                        match res.chunk().await {
                                            Ok(Some(chunk)) => Some((Ok(chunk), Some(res))),
                                            Ok(None) => None,
                                            Err(e) => {
                                                Some((Err(eprintln!("{}", anyhow!(e))), None))
                                            }
                                        }
                                    } else {
                                        None
                                    }
                                })
                            })
                            .try_flatten()
                            .map(move |bytes| {
                                bytes
                                    .and_then(|b| {
                                        pb.inc(b.len() as u64);
                                        sender
                                            .unbounded_send(b)
                                            .map_err(|e| eprintln!("{}", anyhow!(e)))
                                    })
                                    .ok();
                                future::ready(())
                            })
                            .buffered(max_future_per_video)
                            .for_each(|_| future::ready(()))
                            .await;
                    }
                    pb.finish();
                });
            }
            (_, _) => {
                eprintln!("Couldn't get video properly");
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

async fn data_send(out_file: std::path::PathBuf) -> mpsc::UnboundedSender<Bytes> {
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

        while let Some(b) = receiver.next().await {
            loop {
                if let Ok(_) = appsrc.push_buffer(gst::buffer::Buffer::from_slice(b)) {
                    break;
                } else {
                    unreachable!();
                }
            }
        }
        appsrc.end_of_stream().unwrap();

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
    });

    sender
}
