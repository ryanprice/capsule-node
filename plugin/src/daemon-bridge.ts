import { requestUrl, RequestUrlResponse } from "obsidian";

export interface DaemonStatus {
	running: boolean;
	uptime_seconds: number;
	vault_path: string;
	version: string;
	/** Added in slice 5a. Optional in the type so a newer plugin talking
	 * to an older daemon still parses the common fields. */
	keyring?: KeyringState;
	/** EIP-55 Ethereum address; only present when keyring is "unlocked". */
	wallet_address?: string;
}

export type KeyringState = "none" | "locked" | "unlocked";

export type PingResult =
	| { ok: true; data: DaemonStatus }
	| { ok: false; reason: "unreachable" | "bad_response" };

export type KeyringCallResult =
	| { ok: true; state: KeyringState }
	| { ok: false; reason: KeyringFailureReason; message?: string };

export type KeyringFailureReason =
	| "unreachable"
	| "bad_passphrase"
	| "already_exists"
	| "not_found"
	| "bad_request"
	| "server_error";

export class DaemonBridge {
	constructor(private port: number) {}

	setPort(port: number): void {
		this.port = port;
	}

	async pingStatus(): Promise<PingResult> {
		const response = await this.get("/api/v1/status");
		if (!response) return { ok: false, reason: "unreachable" };
		if (response.status < 200 || response.status >= 300) {
			return { ok: false, reason: "bad_response" };
		}
		try {
			const data = response.json as DaemonStatus;
			if (
				typeof data.running !== "boolean" ||
				typeof data.uptime_seconds !== "number"
			) {
				return { ok: false, reason: "bad_response" };
			}
			return { ok: true, data };
		} catch {
			return { ok: false, reason: "bad_response" };
		}
	}

	async getKeyringStatus(): Promise<KeyringCallResult> {
		const response = await this.get("/api/v1/keyring/status");
		if (!response) return { ok: false, reason: "unreachable" };
		return this.parseKeyringResponse(response);
	}

	async initKeyring(passphrase: string): Promise<KeyringCallResult> {
		return this.postPassphrase("/api/v1/keyring/init", passphrase);
	}

	async unlockKeyring(passphrase: string): Promise<KeyringCallResult> {
		return this.postPassphrase("/api/v1/keyring/unlock", passphrase);
	}

	async lockKeyring(): Promise<KeyringCallResult> {
		const response = await this.post("/api/v1/keyring/lock", "");
		if (!response) return { ok: false, reason: "unreachable" };
		return this.parseKeyringResponse(response);
	}

	private async postPassphrase(
		path: string,
		passphrase: string,
	): Promise<KeyringCallResult> {
		const body = JSON.stringify({ passphrase });
		const response = await this.post(path, body);
		if (!response) return { ok: false, reason: "unreachable" };
		return this.parseKeyringResponse(response);
	}

	private parseKeyringResponse(response: RequestUrlResponse): KeyringCallResult {
		if (response.status >= 200 && response.status < 300) {
			try {
				const data = response.json as { status?: KeyringState };
				if (
					data.status === "none" ||
					data.status === "locked" ||
					data.status === "unlocked"
				) {
					return { ok: true, state: data.status };
				}
			} catch {
				// fall through
			}
			return { ok: false, reason: "server_error", message: "bad response shape" };
		}
		const message =
			(() => {
				try {
					return (response.json as { error?: string })?.error;
				} catch {
					return undefined;
				}
			})() ?? undefined;
		const reason = ((): KeyringFailureReason => {
			switch (response.status) {
				case 401:
					return "bad_passphrase";
				case 409:
					return "already_exists";
				case 404:
					return "not_found";
				case 400:
					return "bad_request";
				default:
					return "server_error";
			}
		})();
		return { ok: false, reason, message };
	}

	private async get(path: string): Promise<RequestUrlResponse | null> {
		const url = `http://127.0.0.1:${this.port}${path}`;
		try {
			return await requestUrl({ url, method: "GET", throw: false });
		} catch {
			return null;
		}
	}

	private async post(
		path: string,
		body: string,
	): Promise<RequestUrlResponse | null> {
		const url = `http://127.0.0.1:${this.port}${path}`;
		try {
			return await requestUrl({
				url,
				method: "POST",
				body,
				contentType: "application/json",
				throw: false,
			});
		} catch {
			return null;
		}
	}
}
