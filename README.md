# hls-downloader
It's HLS Downloader written in Rust. It uses a `gstreamer` to merge video chunks downloaded from a HLS playlist, so before building it, you should install gstreamer library (as well as development version) properly.

It uses multi-threading and asyncronous I/O. In some services, therefore, its downloading speed could be faster than sequential alternatives.

This downloader has a feature downloading a part of entire media. For example, it could download first or last 10 minutes of a video, or from 1:30:00 to 3:00:00. To check full command-line options, try `hls-downloader --help`.
