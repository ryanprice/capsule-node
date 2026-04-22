import { App, Notice, PluginSettingTab, Setting } from "obsidian";
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

		const keyringSetting = new Setting(containerEl).setName("Keyring status").setDesc("Loading…");

		// Mutable ref captured by both the refresh path (which updates it) and
		// the copy button's click handler (which reads the current value).
		// A closure over a local won't work — addExtraButton's callback captures
		// the variable at registration time, not at click time.
		const walletRef: { current: string | null } = { current: null };

		const walletSetting = new Setting(containerEl)
			.setName("Wallet address")
			.setDesc("Loading…")
			.addExtraButton((btn) =>
				btn
					.setIcon("copy")
					.setTooltip("Copy address to clipboard")
					.onClick(() => {
						void this.copyWalletAddress(walletRef.current);
					}),
			);

		new Setting(containerEl).addButton((btn) =>
			btn
				.setButtonText("Refresh")
				.setTooltip("Re-query the daemon for current keyring + wallet state.")
				.onClick(() => {
					void this.refreshDaemonState(keyringSetting, walletSetting, walletRef);
				}),
		);

		void this.refreshDaemonState(keyringSetting, walletSetting, walletRef);
	}

	private async refreshDaemonState(
		keyringSetting: Setting,
		walletSetting: Setting,
		walletRef: { current: string | null },
	): Promise<void> {
		keyringSetting.setDesc("Loading…");
		walletSetting.setDesc("Loading…");
		walletRef.current = null;

		const result = await this.plugin.bridge.pingStatus();
		if (!result.ok) {
			const message = `Daemon unreachable (${result.reason}).`;
			keyringSetting.setDesc(message);
			walletSetting.setDesc(message);
			return;
		}

		const keyring = result.data.keyring ?? "unknown";
		keyringSetting.setDesc(describeKeyring(keyring));

		const addr = result.data.wallet_address;
		if (addr) {
			walletRef.current = addr;
			walletSetting.setDesc(renderWalletAddress(addr));
		} else {
			walletRef.current = null;
			walletSetting.setDesc("No address — unlock the keyring first.");
		}
	}

	private async copyWalletAddress(addr: string | null): Promise<void> {
		if (!addr) {
			new Notice("No wallet address to copy yet.");
			return;
		}
		try {
			await navigator.clipboard.writeText(addr);
			new Notice("Wallet address copied.");
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			new Notice(`Copy failed: ${message}`);
		}
	}
}

/**
 * Render an Ethereum address as a monospace, user-selectable element.
 * Obsidian's Setting.setDesc accepts a DocumentFragment, so a <code> node
 * with explicit `user-select: text` survives any parent CSS that would
 * otherwise block text selection in description rows.
 */
function renderWalletAddress(addr: string): DocumentFragment {
	const frag = document.createDocumentFragment();
	const code = frag.createEl("code", { text: addr });
	code.style.userSelect = "text";
	code.style.cursor = "text";
	code.style.fontSize = "0.95em";
	return frag;
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
