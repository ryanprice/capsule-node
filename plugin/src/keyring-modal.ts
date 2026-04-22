import { App, Modal, Setting } from "obsidian";
import { DaemonBridge, KeyringCallResult } from "./daemon-bridge";

/**
 * Shared between InitKeyringModal and UnlockKeyringModal. Both collect a
 * passphrase, submit it to the daemon, show inline errors on failure, and
 * resolve with the daemon's response when closed.
 *
 * Security notes:
 *  * Inputs use type="password" so the passphrase isn't shoulder-visible.
 *  * The input values are explicitly cleared after submit (set to "") and
 *    overwritten to "" before the modal closes, so the DOM doesn't hold
 *    the passphrase after the user is done.
 *  * We don't log the passphrase anywhere.
 */

interface ModalConfig {
	title: string;
	intro: string;
	submitLabel: string;
	confirmPassphrase: boolean;
	action: (bridge: DaemonBridge, passphrase: string) => Promise<KeyringCallResult>;
}

class KeyringPassphraseModal extends Modal {
	private passphraseInput?: HTMLInputElement;
	private confirmInput?: HTMLInputElement;
	private errorEl?: HTMLElement;
	private submitting = false;

	constructor(
		app: App,
		private bridge: DaemonBridge,
		private config: ModalConfig,
		private onResolve: (result: KeyringCallResult | null) => void,
	) {
		super(app);
	}

	onOpen(): void {
		const { contentEl } = this;
		contentEl.empty();
		contentEl.createEl("h2", { text: this.config.title });
		contentEl.createEl("p", { text: this.config.intro });

		this.passphraseInput = contentEl.createEl("input", {
			attr: {
				type: "password",
				placeholder: "Passphrase",
				autocomplete: "new-password",
				style: "width: 100%; margin-bottom: 0.5em;",
			},
		});

		if (this.config.confirmPassphrase) {
			this.confirmInput = contentEl.createEl("input", {
				attr: {
					type: "password",
					placeholder: "Confirm passphrase",
					autocomplete: "new-password",
					style: "width: 100%; margin-bottom: 0.5em;",
				},
			});
		}

		this.errorEl = contentEl.createEl("div", {
			attr: { style: "color: var(--text-error); min-height: 1.2em; margin-bottom: 0.5em;" },
		});

		new Setting(contentEl).addButton((btn) =>
			btn
				.setButtonText(this.config.submitLabel)
				.setCta()
				.onClick(() => {
					void this.submit();
				}),
		);

		// Submit on Enter from either field.
		for (const input of [this.passphraseInput, this.confirmInput]) {
			input?.addEventListener("keydown", (ev) => {
				if (ev.key === "Enter") {
					ev.preventDefault();
					void this.submit();
				}
			});
		}

		this.passphraseInput.focus();
	}

	onClose(): void {
		// Defense-in-depth: clear any passphrase left in the DOM.
		if (this.passphraseInput) this.passphraseInput.value = "";
		if (this.confirmInput) this.confirmInput.value = "";
		this.contentEl.empty();
	}

	private async submit(): Promise<void> {
		if (this.submitting) return;
		if (!this.passphraseInput) return;

		const passphrase = this.passphraseInput.value;
		const confirmation = this.confirmInput?.value;

		if (!passphrase) {
			this.showError("Passphrase must not be empty.");
			return;
		}
		if (this.config.confirmPassphrase && passphrase !== confirmation) {
			this.showError("Passphrases do not match.");
			return;
		}

		this.submitting = true;
		this.showError("");

		const result = await this.config.action(this.bridge, passphrase);

		// Clear immediately — before resolving or closing.
		this.passphraseInput.value = "";
		if (this.confirmInput) this.confirmInput.value = "";

		if (result.ok) {
			this.onResolve(result);
			this.close();
		} else {
			this.showError(friendlyError(result));
			this.submitting = false;
		}
	}

	private showError(msg: string): void {
		if (this.errorEl) this.errorEl.setText(msg);
	}
}

function friendlyError(result: Extract<KeyringCallResult, { ok: false }>): string {
	switch (result.reason) {
		case "unreachable":
			return "Daemon unreachable. Is capsuled running?";
		case "bad_passphrase":
			return "Bad passphrase or corrupted keyring.";
		case "already_exists":
			return "A keyring already exists on disk.";
		case "not_found":
			return "No keyring file on disk.";
		case "bad_request":
			return result.message ?? "Bad request.";
		case "server_error":
		default:
			return result.message ?? "Daemon reported an internal error.";
	}
}

export function promptInitKeyring(
	app: App,
	bridge: DaemonBridge,
): Promise<KeyringCallResult | null> {
	return new Promise((resolve) => {
		new KeyringPassphraseModal(
			app,
			bridge,
			{
				title: "Initialize Capsule keyring",
				intro:
					"Choose a passphrase to encrypt your node identity. You'll need this passphrase each time the daemon starts.",
				submitLabel: "Initialize",
				confirmPassphrase: true,
				action: (b, p) => b.initKeyring(p),
			},
			resolve,
		).open();
	});
}

export function promptUnlockKeyring(
	app: App,
	bridge: DaemonBridge,
): Promise<KeyringCallResult | null> {
	return new Promise((resolve) => {
		new KeyringPassphraseModal(
			app,
			bridge,
			{
				title: "Unlock Capsule keyring",
				intro: "Enter the passphrase you set when initializing the keyring.",
				submitLabel: "Unlock",
				confirmPassphrase: false,
				action: (b, p) => b.unlockKeyring(p),
			},
			resolve,
		).open();
	});
}
