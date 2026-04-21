import { Notice, Plugin } from "obsidian";
import { DaemonBridge } from "./daemon-bridge";
import { CapsuleNodeSettings, CapsuleNodeSettingTab, DEFAULT_SETTINGS } from "./settings";

export default class CapsuleNodePlugin extends Plugin {
	settings!: CapsuleNodeSettings;
	bridge!: DaemonBridge;

	async onload(): Promise<void> {
		await this.loadSettings();
		this.bridge = new DaemonBridge(this.settings.daemonPort);

		this.addRibbonIcon("plug-zap", "Check Capsule daemon status", () => {
			void this.checkDaemonStatus();
		});

		this.addCommand({
			id: "check-daemon-status",
			name: "Check daemon status",
			callback: () => {
				void this.checkDaemonStatus();
			},
		});

		this.addSettingTab(new CapsuleNodeSettingTab(this.app, this));
	}

	onunload(): void {
		// Intentional: do NOT stop the daemon. It runs independently (spec §3.2).
	}

	async loadSettings(): Promise<void> {
		this.settings = Object.assign({}, DEFAULT_SETTINGS, await this.loadData());
	}

	async saveSettings(): Promise<void> {
		await this.saveData(this.settings);
	}

	private async checkDaemonStatus(): Promise<void> {
		const result = await this.bridge.pingStatus();
		if (result.ok) {
			const { uptime_seconds, version } = result.data;
			new Notice(`Capsule daemon v${version} — up ${formatUptime(uptime_seconds)}`);
		} else {
			new Notice(`Capsule daemon unavailable (${result.reason})`);
		}
	}
}

function formatUptime(seconds: number): string {
	if (seconds < 60) return `${seconds}s`;
	if (seconds < 3600) return `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
	const h = Math.floor(seconds / 3600);
	const m = Math.floor((seconds % 3600) / 60);
	return `${h}h ${m}m`;
}
