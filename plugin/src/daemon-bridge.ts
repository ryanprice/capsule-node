import { requestUrl, RequestUrlResponse } from "obsidian";

export interface DaemonStatus {
	running: boolean;
	uptime_seconds: number;
	vault_path: string;
	version: string;
}

export type PingResult =
	| { ok: true; data: DaemonStatus }
	| { ok: false; reason: "unreachable" | "bad_response" };

export class DaemonBridge {
	constructor(private port: number) {}

	setPort(port: number): void {
		this.port = port;
	}

	async pingStatus(): Promise<PingResult> {
		const url = `http://127.0.0.1:${this.port}/api/v1/status`;
		let response: RequestUrlResponse;
		try {
			// Use Obsidian's requestUrl — bypasses browser CORS/mixed-content
			// restrictions that block raw fetch() against localhost services.
			response = await requestUrl({ url, method: "GET", throw: false });
		} catch {
			return { ok: false, reason: "unreachable" };
		}

		if (response.status < 200 || response.status >= 300) {
			return { ok: false, reason: "bad_response" };
		}

		try {
			const data = response.json as DaemonStatus;
			if (typeof data.running !== "boolean" || typeof data.uptime_seconds !== "number") {
				return { ok: false, reason: "bad_response" };
			}
			return { ok: true, data };
		} catch {
			return { ok: false, reason: "bad_response" };
		}
	}
}
