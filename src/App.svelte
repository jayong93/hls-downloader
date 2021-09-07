<script>
	import { invoke } from "@tauri-apps/api/tauri";
	import { listen } from "@tauri-apps/api/event";
	import { onDestroy } from "svelte";

	import Adder from "./adder.svelte";
	import VideoList from "./download_list.svelte";

	let video_list = [];
	let log_msg = [];

	function download() {
		let arg = video_list.map((v) => {
			v.selected_bandwidth = v.selected_bandwidth.idx;
			return v;
		});
		invoke("download", { videoList: arg }).finally(() => (video_list = []));
	}

	const unlisten = listen("AddLog", (event) => {
		log_msg = [...log_msg, event.payload];
	});

	$: total_log = log_msg && log_msg.length > 0 ? log_msg.join("\n") : "";

	onDestroy(() => unlisten());
</script>

<main>
	<Adder
		on:Add={(message) => {
			video_list = [...video_list, message.detail];
		}}
	/>
	<VideoList bind:list={video_list} />
	<div>
		<textarea class="log" value={total_log} readonly />
	</div>
	<div>
		<button on:click={download}>Download All</button>
	</div>
</main>

<style>
	main {
		text-align: center;
		padding: 1em;
		width: 100%;
		margin: 0 auto;
	}

	@media (min-width: 640px) {
		main {
			max-width: none;
		}
	}

	.log {
		overflow-y: scroll;
		width: 90%;
		height: 100%;
		resize: none;
	}
</style>
