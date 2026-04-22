import { debounce, Notice, Plugin, TAbstractFile, TFile } from "obsidian";
import { CapsuleManager } from "./capsule-manager";
import { DaemonBridge, KeyringCallResult } from "./daemon-bridge";
import { CapsuleDecorator } from "./decorator";
import { FrontmatterError } from "./frontmatter";
import { promptInitKeyring, promptUnlockKeyring } from "./keyring-modal";
import { CapsuleNodeSettings, CapsuleNodeSettingTab, DEFAULT_SETTINGS } from "./settings";

const SYNC_DEBOUNCE_MS = 400;
const FIRST_RUN_NOTICE_DELAY_MS = 1500;

export default class CapsuleNodePlugin extends Plugin {
	settings!: CapsuleNodeSettings;
	bridge!: DaemonBridge;
	capsules!: CapsuleManager;
	decorator!: CapsuleDecorator;

	async onload(): Promise<void> {
		await this.loadSettings();
		this.bridge = new DaemonBridge(this.settings.daemonPort);
		this.capsules = new CapsuleManager(this.app, () => this.settings.capsuleFolder);
		this.decorator = new CapsuleDecorator(this, this.app, () => this.settings.capsuleFolder);
		this.decorator.register();

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

		this.addCommand({
			id: "create-draft-capsule",
			name: "Create draft capsule",
			callback: () => {
				void this.createDraftCapsule();
			},
		});

		this.addCommand({
			id: "init-keyring",
			name: "Initialize keyring",
			callback: () => {
				void this.runInitKeyring();
			},
		});

		this.addCommand({
			id: "unlock-keyring",
			name: "Unlock keyring",
			callback: () => {
				void this.runUnlockKeyring();
			},
		});

		this.addCommand({
			id: "lock-keyring",
			name: "Lock keyring",
			callback: () => {
				void this.runLockKeyring();
			},
		});

		const debouncedSync = debounce(
			(file: TFile) => {
				void this.syncNoteToManifest(file);
			},
			SYNC_DEBOUNCE_MS,
			true,
		);

		this.registerEvent(
			this.app.vault.on("modify", (file: TAbstractFile) => {
				if (!(file instanceof TFile)) return;
				if (!this.capsules.isCapsuleNotePath(file.path)) return;
				debouncedSync(file);
			}),
		);

		this.addSettingTab(new CapsuleNodeSettingTab(this.app, this));

		// One-time nudge on load: if the daemon is reachable but the keyring
		// isn't Unlocked, tell the user which command to run. Deferred so the
		// daemon has a moment to finish starting after a `dev.sh` restart.
		setTimeout(() => {
			void this.maybeShowFirstRunNotice();
		}, FIRST_RUN_NOTICE_DELAY_MS);
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

	private async createDraftCapsule(): Promise<void> {
		try {
			const { file, manifest } = await this.capsules.createDraftCapsule();
			await this.app.workspace.getLeaf(true).openFile(file);
			new Notice(
				`Created draft capsule ${manifest.capsule_id}. Edit the note, then flip status to active.`
			);
		} catch (err) {
			const message = err instanceof Error ? err.message : String(err);
			new Notice(`Failed to create capsule: ${message}`);
		}
	}

	private async runInitKeyring(): Promise<void> {
		const result = await promptInitKeyring(this.app, this.bridge);
		this.reportKeyringResult("Keyring initialized", result);
	}

	private async runUnlockKeyring(): Promise<void> {
		const result = await promptUnlockKeyring(this.app, this.bridge);
		this.reportKeyringResult("Keyring unlocked", result);
	}

	private async runLockKeyring(): Promise<void> {
		const result = await this.bridge.lockKeyring();
		this.reportKeyringResult("Keyring locked", result);
	}

	private reportKeyringResult(
		successPrefix: string,
		result: KeyringCallResult | null,
	): void {
		if (!result) return; // user dismissed the modal
		if (result.ok) {
			new Notice(`${successPrefix} (state: ${result.state}).`);
		} else {
			new Notice(`Keyring operation failed: ${result.reason}`);
		}
	}

	private async maybeShowFirstRunNotice(): Promise<void> {
		const status = await this.bridge.getKeyringStatus();
		if (!status.ok) return; // daemon unreachable: separate problem, silent here
		if (status.state === "none") {
			new Notice(
				"Capsule Node: no keyring yet. Cmd/Ctrl+P → \"Initialize keyring\" to set up.",
				8000,
			);
		} else if (status.state === "locked") {
			new Notice(
				"Capsule Node: keyring is locked. Cmd/Ctrl+P → \"Unlock keyring\" to continue.",
				8000,
			);
		}
	}

	private async syncNoteToManifest(file: TFile): Promise<void> {
		try {
			await this.capsules.syncNoteToManifest(file);
		} catch (err) {
			// User is mid-edit — don't pop a Notice on every keystroke's worth
			// of broken YAML. Console-only; they'll see Notices when they use
			// a command explicitly.
			if (err instanceof FrontmatterError) {
				console.warn(
					`Capsule note ${file.path} could not be parsed: ${err.message}`,
				);
				return;
			}
			console.error(
				`Failed to sync capsule note ${file.path} to manifest:`,
				err,
			);
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
