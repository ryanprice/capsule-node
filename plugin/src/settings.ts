import { App, PluginSettingTab, Setting } from "obsidian";
import type CapsuleNodePlugin from "./main";

export interface CapsuleNodeSettings {
	daemonPort: number;
	capsuleFolder: string;
}

export const DEFAULT_SETTINGS: CapsuleNodeSettings = {
	daemonPort: 7402,
	capsuleFolder: "Capsules",
};

export class CapsuleNodeSettingTab extends PluginSettingTab {
	constructor(app: App, private plugin: CapsuleNodePlugin) {
		super(app, plugin);
	}

	display(): void {
		const { containerEl } = this;
		containerEl.empty();

		containerEl.createEl("h2", { text: "Capsule Node" });

		new Setting(containerEl)
			.setName("Daemon port")
			.setDesc("Localhost port the companion daemon serves its management API on.")
			.addText((text) =>
				text
					.setPlaceholder("7402")
					.setValue(String(this.plugin.settings.daemonPort))
					.onChange(async (value) => {
						const parsed = Number.parseInt(value, 10);
						if (Number.isFinite(parsed) && parsed > 0 && parsed <= 65535) {
							this.plugin.settings.daemonPort = parsed;
							this.plugin.bridge.setPort(parsed);
							await this.plugin.saveSettings();
						}
					})
			);

		new Setting(containerEl)
			.setName("Capsule folder")
			.setDesc("Folder inside the vault where capsule notes live.")
			.addText((text) =>
				text
					.setPlaceholder("Capsules")
					.setValue(this.plugin.settings.capsuleFolder)
					.onChange(async (value) => {
						const trimmed = value.trim().replace(/\/+$/, "");
						this.plugin.settings.capsuleFolder = trimmed || "Capsules";
						await this.plugin.saveSettings();
					})
			);

		new Setting(containerEl).setName("Vault path").setDesc(vaultPath(this.app)).setDisabled(true);

		// ── Daemon-reported state ──────────────────────────────────────────
		// Two read-only rows that pull from the mgmt /api/v1/status endpoint.
		// Values are fetched async; we seed them with a "Loading…" placeholder
		// and populate when the request comes back. A Refresh button re-runs
		// the fetch without reopening the settings tab.
		containerEl.createEl("h3", { text: "Node identity" });

		const keyringSetting = new Setting(containerEl)
			.setName("Keyring status")
			.setDesc("Loading…")
			.setDisabled(true);

		const walletSetting = new Setting(containerEl)
			.setName("Wallet address")
			.setDesc("Loading…")
			.setDisabled(true);

		new Setting(containerEl).addButton((btn) =>
			btn
				.setButtonText("Refresh")
				.setTooltip("Re-query the daemon for current keyring + wallet state.")
				.onClick(() => {
					void this.refreshDaemonState(keyringSetting, walletSetting);
				}),
		);

		void this.refreshDaemonState(keyringSetting, walletSetting);
	}

	private async refreshDaemonState(
		keyringSetting: Setting,
		walletSetting: Setting,
	): Promise<void> {
		keyringSetting.setDesc("Loading…");
		walletSetting.setDesc("Loading…");

		const result = await this.plugin.bridge.pingStatus();
		if (!result.ok) {
			const message = `Daemon unreachable (${result.reason}).`;
			keyringSetting.setDesc(message);
			walletSetting.setDesc(message);
			return;
		}

		const keyring = result.data.keyring ?? "unknown";
		keyringSetting.setDesc(describeKeyring(keyring));
		walletSetting.setDesc(
			result.data.wallet_address ?? "No address — unlock the keyring first.",
		);
	}
}

function describeKeyring(state: string): string {
	switch (state) {
		case "none":
			return "None — run \"Capsule Node: Initialize keyring\".";
		case "locked":
			return "Locked — run \"Capsule Node: Unlock keyring\".";
		case "unlocked":
			return "Unlocked.";
		default:
			return `Unknown (${state}).`;
	}
}

function vaultPath(app: App): string {
	// FileSystemAdapter exposes basePath; other adapters (mobile) don't.
	// Desktop-only plugin, so this is the expected case.
	const adapter = app.vault.adapter as unknown as { basePath?: string };
	return adapter.basePath ?? "(unknown — non-filesystem adapter)";
}
