interface Bandwidth {
    idx: number;
    bandwidth: number;
}

export interface DownloadableVideo {
    hls_url: string;
    bandwidths: Bandwidth[];
    range_start: string;
    range_end: string;
    file_name: string;
    selected_bandwidth?: Bandwidth;
}