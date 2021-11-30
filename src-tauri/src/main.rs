#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use std::{cmp::Ordering, collections::BinaryHeap, path::PathBuf};

use bytes::Bytes;
use futures::prelude::*;
use lazy_static::lazy_static;
use m3u8_rs::{
  parse_playlist_res,
  playlist::{MediaSegment, Playlist},
};
use reqwest::{Client, Url};
use serde::Deserialize;
use tauri::{self, AppHandle, Manager, Wry};
use tokio::sync::{mpsc, oneshot};

lazy_static! {
  static ref REQ_CLIENT: Client = Client::new();
  static ref TIME_PATTERN: regex::Regex =
    regex::Regex::new(r"^\s*(?:(?:(?:(\d+):)?(?:(\d+):))?(\d+))?\s*$").unwrap();
}
static mut APP_HANDLE: Option<AppHandle<Wry>> = None;
const MAX_FUTURE_NUM: usize = 10;

async fn get_hls_playlist(url: &Url) -> Result<Playlist, String> {
  let bytes = REQ_CLIENT
    .get(url.clone())
    .send()
    .and_then(|res| res.bytes())
    .await
    .map_err(|e| e.to_string())?;

  Ok(parse_playlist_res(bytes.as_ref()).map_err(|_| String::from("invalid hls playlist"))?)
}

async fn master_to_media(
  org_url: &Url,
  mut master_list: m3u8_rs::playlist::MasterPlaylist,
  idx: usize,
) -> (Url, m3u8_rs::playlist::MediaPlaylist) {
  master_list
    .variants
    .sort_by_key(|v| v.bandwidth.parse::<usize>().unwrap());
  let target_url = &master_list.variants[idx].uri;
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
      .map_ok(|b| m3u8_rs::parse_media_playlist(b.as_ref()).unwrap().1)
      .map(|res| res.unwrap())
      .await,
  )
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

fn parse_time_range(range_start: Option<&String>, range_end: Option<&String>) -> (f32, f32) {
  let mut ranges = [0.0f32, 0.0];
  for (i, val) in IntoIterator::into_iter([range_start, range_end]).enumerate() {
    if let Some(s) = val {
      let caps = TIME_PATTERN.captures(s.trim()).unwrap();
      let times: Vec<usize> = caps
        .iter()
        .skip(1)
        .map(|m| m.and_then(|mat| mat.as_str().parse().ok()).unwrap_or(0))
        .collect();
      ranges[i] = (times[0] * 3600 + times[1] * 60 + times[2]) as f32;
    }
  }

  (ranges[0], ranges[1])
}

struct NonCopyable<T>(T);

async fn download_video<'a>(
  url: Url,
  mut out_path: PathBuf,
  range_start: Option<String>,
  range_end: Option<String>,
  playlist: m3u8_rs::playlist::MediaPlaylist,
) {
  let (start_time, end_time) = parse_time_range(range_start.as_ref(), range_end.as_ref());
  let contents_list = get_contents_list(playlist, start_time, end_time);

  assert!(out_path.file_name().is_some());
  std::fs::create_dir_all(out_path.parent().unwrap()).unwrap();
  if let None = out_path.extension() {
    out_path.set_extension("ts");
  }

  let (comp_send, comp_recv) = oneshot::channel();
  {
    let sender = data_send(out_path, comp_send).await;
    stream::iter(contents_list)
      .map(|(idx, chunk)| {
        let idx_move = NonCopyable(idx); // Copy 때문에 일어나는 referencing을 제거하기 위한 꼼수
        async {
          let chunk = chunk;
          let idx = idx_move;
          let res = REQ_CLIENT
            .get(url.join(&chunk.uri).unwrap())
            .send()
            .map_err(|e| format!("{}", e))
            .await?;

          let bytes = res.bytes().map_err(|e| format!("{}", e)).await?;
          sender.send((idx.0, bytes)).map_err(|e| format!("{}", e))
        }
      })
      .buffer_unordered(MAX_FUTURE_NUM)
      .for_each(|_| future::ready(()))
      .await;
  }
  comp_recv.await.ok();
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
  use gstreamer as gst;
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
          println!("{:?}", e);
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

#[tauri::command]
fn get_save_file_name() -> Result<String, ()> {
  let dialog = rfd::FileDialog::new().add_filter("Video File", &["ts"]);
  dialog
    .save_file()
    .ok_or(())
    .map(|path| path.to_string_lossy().into())
}

#[tauri::command]
async fn add_video(video_url: String) -> Result<Vec<usize>, String> {
  let url = url::Url::parse(&video_url.trim()).map_err(|e| e.to_string())?;
  let playlist = get_hls_playlist(&url).await?;

  match playlist {
    Playlist::MasterPlaylist(list) => {
      let mut result: Vec<_> = list
        .variants
        .into_iter()
        .map(|stream| stream.bandwidth.parse::<usize>().unwrap())
        .collect();
      result.sort();
      Ok(result)
    }
    Playlist::MediaPlaylist(_) => Ok(vec![]),
  }
}

#[derive(Debug, Deserialize)]
struct Bandwidth {
  idx: usize,
  bandwidth: usize,
}

#[derive(Debug, Deserialize)]
struct HLSVideo {
  #[serde(rename = "hls_url")]
  url: String,
  file_name: String,
  range_start: Option<String>,
  range_end: Option<String>,
  selected_bandwidth: Option<Bandwidth>,
}

#[tauri::command]
async fn download(video_list: Vec<HLSVideo>) {
  let app_handle = unsafe { APP_HANDLE.as_ref() }.unwrap();
  for video in video_list {
    app_handle
      .emit_all("AddLog", format!("Downloading {}", video.file_name))
      .ok();
    if let Some(url) = url::Url::parse(&video.url.trim())
      .map_err(|e| app_handle.emit_all("AddLog", e.to_string()))
      .ok()
    {
      match get_hls_playlist(&url).await {
        Ok(playlist) => match playlist {
          Playlist::MasterPlaylist(list) => {
            let (new_url, media) = master_to_media(
              &url,
              list,
              video
                .selected_bandwidth
                .map(|Bandwidth { idx, bandwidth: _ }| idx)
                .unwrap_or(0),
            )
            .await;
            download_video(
              new_url,
              video.file_name.clone().into(),
              video.range_start,
              video.range_end,
              media,
            )
            .await;
          }
          Playlist::MediaPlaylist(media) => {
            download_video(
              url,
              video.file_name.clone().into(),
              video.range_start,
              video.range_end,
              media,
            )
            .await;
          }
        },
        Err(err) => {
          app_handle.emit_all("AddLog", err.to_string()).ok();
        }
      }
    }
    app_handle
      .emit_all("AddLog", format!("Downloaded {}", video.file_name))
      .ok();
  }
}

fn main() {
  tauri::Builder::default()
    .setup(|app| {
      unsafe {
        APP_HANDLE = Some(app.handle());
      }
      gstreamer::init().unwrap();
      Ok(())
    })
    .invoke_handler(tauri::generate_handler![
      get_save_file_name,
      add_video,
      download
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
