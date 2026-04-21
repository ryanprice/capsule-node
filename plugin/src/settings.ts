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
	}
}

function vaultPath(app: App): string {
	// FileSystemAdapter exposes basePath; other adapters (mobile) don't.
	// Desktop-only plugin, so this is the expected case.
	const adapter = app.vault.adapter as unknown as { basePath?: string };
	return adapter.basePath ?? "(unknown — non-filesystem adapter)";
}
