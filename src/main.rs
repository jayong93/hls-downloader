#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use std::{cmp::Ordering, collections::BinaryHeap, path::PathBuf};

use bytes::Bytes;
use clap::Parser;
use futures::prelude::*;
use gstreamer as gst;
use indicatif::{self, ProgressBar, ProgressStyle};
use lazy_static::lazy_static;
use log::{debug, error};
use m3u8_rs::{
  parse_playlist_res,
  playlist::{MediaSegment, Playlist},
};
use pretty_env_logger;
use reqwest::{Client, Url};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot};

lazy_static! {
  static ref REQ_CLIENT: Client = Client::new();
  static ref TIME_PATTERN: regex::Regex =
    regex::Regex::new(r"^\s*(?:(?:(?:(\d+):)?(?:(\d+):))?(\d+))?\s*$").unwrap();
}
const MAX_FUTURE_NUM: usize = 10;

async fn get_hls_playlist(url: &Url) -> Option<Playlist> {
  let bytes = REQ_CLIENT
    .get(url.clone())
    .send()
    .and_then(|res| res.bytes())
    .await
    .map_err(|e| error!("couldn't get a HLS playlist from the url. url: {url:?}, error: {e:?}"))
    .ok()?;

  parse_playlist_res(bytes.as_ref())
    .map_err(|_| error!("invalid hls playlist: {url:?}"))
    .ok()
}

async fn master_to_media(
  org_url: &Url,
  mut master_list: m3u8_rs::playlist::MasterPlaylist,
) -> Option<(Url, m3u8_rs::playlist::MediaPlaylist)> {
  master_list
    .variants
    .sort_by_key(|v| v.bandwidth.parse::<usize>().unwrap());
  let target_url = &master_list
    .variants
    .last()
    .or_else(|| {
      error!("master playlist doesn't have bandwidth variants: {master_list:?}");
      None
    })?
    .uri;
  let media_url = Url::parse(target_url)
    .and_then(|u| {
      if !u.cannot_be_a_base() {
        Ok(u)
      } else {
        org_url.join(target_url)
      }
    })
    .or(org_url.join(target_url))
    .map(|mut u| {
      u.set_query(org_url.query());
      u
    }) // append query params of the org_url to the target url
    .map_err(|e| error!("failed to parse media playlist url. url: {target_url:?}, error: {e:?}"))
    .ok()?;
  Some((
    media_url.clone(),
    REQ_CLIENT
      .get(media_url)
      .send()
      .map_err(|e| error!("couldn't send a request. error: {e}"))
      .and_then(|res| {
        res
          .bytes()
          .map_err(|e| error!("couldn't get a response's body. error: {e}"))
      })
      .await
      .and_then(|b| {
        m3u8_rs::parse_media_playlist(b.as_ref())
          .map(|res| res.1)
          .map_err(|e| error!("couldn't parse HLS media playlist. error: {e:?}"))
      })
      .ok()?,
  ))
}

fn get_contents_list(
  playlist: m3u8_rs::playlist::MediaPlaylist,
  start_time: f32,
  end_time: f32,
) -> Vec<(usize, MediaSegment)> {
  let mut cumul_time = 0f32;

  let mut it = playlist.segments.into_iter();
  if start_time > 0.0 {
    while let Some(segment) = it.next() {
      let chunk_end_time = cumul_time + segment.duration;
      if chunk_end_time < start_time {
        cumul_time = chunk_end_time;
      } else {
        break;
      }
    }
  }

  if end_time == 0.0 {
    it.enumerate().collect()
  } else {
    it.take_while(|chunk| {
      if cumul_time <= end_time {
        cumul_time += chunk.duration;
        true
      } else {
        false
      }
    })
    .enumerate()
    .collect()
  }
}

struct NonCopyable<T>(T);

async fn download_video<'a>(
  url: Url,
  mut out_path: PathBuf,
  start_time: f32,
  end_time: f32,
  playlist: m3u8_rs::playlist::MediaPlaylist,
) {
  let contents_list = get_contents_list(playlist, start_time, end_time);
  let contents_len = contents_list.len();

  assert!(out_path.file_name().is_some());
  std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
  if let None = out_path.extension() {
    out_path.set_extension("ts");
  }

  let pb = ProgressBar::new(contents_len as u64).with_message(out_path.to_string_lossy().into_owned());
  pb.set_style(ProgressStyle::default_bar().template("[{msg}][{elapsed}] {wide_bar}"));

  let (comp_send, comp_recv) = oneshot::channel();
  {
    let sender = data_send(out_path, comp_send).await;
    stream::iter(contents_list)
      .map(|(idx, chunk)| {
        let idx_move = NonCopyable(idx); // Copy 때문에 일어나는 referencing을 제거하기 위한 꼼수
        async {
          let chunk = chunk;
          let chunk_url = {
            let mut chunk_url = url.join(&chunk.uri).unwrap();
            chunk_url.set_query(url.query());
            chunk_url
          };
          let idx = idx_move;
          let res = REQ_CLIENT
            .get(chunk_url.clone())
            .send()
            .map_err(|e| error!("couldn't send a request. url: {chunk_url:?}, error: {e:?}"))
            .await?;

          let res_str = format!("{res:?}");

          let bytes = res
            .bytes()
            .map_err(|e| {
              error!("couldn't get a body of a response. res: {res_str:?}, error: {e:?}")
            })
            .await?;
          sender
            .send((idx.0, bytes))
            .map_err(|e| error!("file concat module has exited. error: {e:?}"))?;

          pb.inc(1);

          Result::<(), ()>::Ok(())
        }
      })
      .buffer_unordered(MAX_FUTURE_NUM)
      .for_each(|_| future::ready(()))
      .await;
  }
  comp_recv.await.ok();
  pb.finish();
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
) -> mpsc::UnboundedSender<(usize, Bytes)> {
  use gst::prelude::*;
  use gstreamer_app as gst_app;

  let (sender, mut receiver) = mpsc::unbounded_channel();

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
    while let Some((idx, b)) = receiver.recv().await {
      if idx == cur_idx {
        appsrc
          .push_buffer(gst::buffer::Buffer::from_slice(b))
          .unwrap();
        cur_idx += 1;
      } else {
        heap.push(IndexedByte { idx, data: b });
        match heap.peek() {
          Some(val) if val.idx == cur_idx => {
            appsrc
              .push_buffer(gst::buffer::Buffer::from_slice(heap.pop().unwrap().data))
              .unwrap();
            cur_idx += 1;
          }
          _ => {}
        }
      }
    }
    while !heap.is_empty() {
      appsrc
        .push_buffer(gst::buffer::Buffer::from_slice(heap.pop().unwrap().data))
        .unwrap();
    }
    appsrc.end_of_stream().unwrap();

    let bus = pipeline.bus().unwrap();

    use gst::MessageView;
    for msg in bus.iter_timed(gst::ClockTime::NONE) {
      match msg.view() {
        MessageView::Error(e) => {
          error!("{e:?}");
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

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct Bandwidth {
  idx: usize,
  bandwidth: usize,
}

#[derive(Parser)]
#[clap(author, version, about)]
struct CliOption {
  url: String,
  name: String,
  #[clap(long, parse(try_from_str = parse_time_str), default_value_t = 0f32)]
  start_at: f32,
  #[clap(long, parse(try_from_str = parse_time_str), default_value_t = 0f32)]
  end_at: f32,
}

fn parse_time_str(time: &str) -> Result<f32, String> {
  time
    .split(':')
    .rev()
    .enumerate()
    .take(3)
    .try_fold(0f32, |acc, (idx, s)| {
      Ok(
        acc
          + s
            .parse::<f32>()
            .map_err(|e| format!("couldn't parse time string. error: {e:?}"))?
            * 60f32.powi(idx as i32),
      )
    })
}

#[tokio::main]
async fn main() -> Result<(), ()> {
  gst::init().unwrap();
  pretty_env_logger::init();
  let cli_option = CliOption::parse();

  let url: Url = cli_option
    .url
    .parse()
    .map_err(|e| error!("wrong formatted url. url: {}, error: {e:?}", cli_option.url))?;
  let playlist = get_hls_playlist(&url).await.unwrap();
  let (url, playlist) = match playlist {
    Playlist::MasterPlaylist(master) => master_to_media(&url, master).await.unwrap(),
    Playlist::MediaPlaylist(media) => (url, media),
  };
  debug!("try to download with url: {url:?}");
  download_video(
    url,
    cli_option.name.into(),
    cli_option.start_at,
    cli_option.end_at,
    playlist,
  )
  .await;

  Ok(())
}
