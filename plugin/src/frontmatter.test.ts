import { strict as assert } from "node:assert";
import { describe, it } from "node:test";

import {
	buildCapsuleNote,
	FrontmatterError,
	parseCapsuleNote,
	replaceDaemonZone,
	ZONE_MARKER,
} from "./frontmatter";
import { Manifest } from "./manifest";

function sampleManifest(): Manifest {
	return {
		capsule_id: "cap_abc123",
		schema: "capsule://health.glucose.continuous",
		status: "active",
		floor_price: "0.08 USDC/query",
		computation_classes: ["A", "B"],
		tags: ["glucose", "cgm"],
	};
}

describe("buildCapsuleNote", () => {
	it("emits a note with both zones separated by the marker", () => {
		const note = buildCapsuleNote({ manifest: sampleManifest() });
		assert.ok(note.startsWith("---\n"));
		assert.ok(note.includes(ZONE_MARKER));
		assert.ok(note.includes("\n---\n"));
		// User zone contains capsule_id; daemon zone contains payload_cid.
		const marker = note.indexOf(ZONE_MARKER);
		const userPart = note.slice(0, marker);
		const daemonPart = note.slice(marker);
		assert.ok(userPart.includes("capsule_id: cap_abc123"));
		assert.ok(daemonPart.includes("payload_cid:"));
	});

	it("daemon fields default to null when not provided", () => {
		const note = buildCapsuleNote({ manifest: sampleManifest() });
		const parsed = parseCapsuleNote(note);
		assert.equal(parsed.daemonFields.payload_cid, null);
		assert.equal(parsed.daemonFields.earnings_total, null);
		assert.equal(parsed.daemonFields.queries_served, null);
		assert.equal(parsed.daemonFields.last_accessed, null);
	});
});

describe("parseCapsuleNote", () => {
	it("round-trips a note built by buildCapsuleNote", () => {
		const m = sampleManifest();
		const note = buildCapsuleNote({ manifest: m });
		const parsed = parseCapsuleNote(note);
		// buildCapsuleNote fills in defaults for sources/extraction; the
		// round-tripped manifest has those populated too.
		assert.deepEqual(parsed.manifest, {
			...m,
			sources: [],
			extraction: "none",
		});
	});

	it("round-trips sources + extraction when set", () => {
		const m = {
			...sampleManifest(),
			sources: ["[[raw/cgm-2026-01]]", "raw/cgm-2026-02"],
			extraction: "frontmatter-list" as const,
		};
		const note = buildCapsuleNote({ manifest: m });
		const parsed = parseCapsuleNote(note);
		assert.deepEqual(parsed.manifest.sources, m.sources);
		assert.equal(parsed.manifest.extraction, "frontmatter-list");
	});

	it("rejects invalid extraction mode", () => {
		const bad = [
			"---",
			"capsule_id: cap_abc123",
			"schema: capsule://x",
			"status: draft",
			"floor_price: 0.01",
			"computation_classes: [A]",
			"tags: []",
			"extraction: bogus",
			"---",
			"",
		].join("\n");
		assert.throws(() => parseCapsuleNote(bad), FrontmatterError);
	});

	it("round-trips daemon fields when present", () => {
		const m = sampleManifest();
		const note = buildCapsuleNote({
			manifest: m,
			daemonFields: {
				payload_cid: "bafy123",
				earnings_total: "12.34 USDC",
				queries_served: 42,
				last_accessed: "2026-04-20T12:00:00Z",
			},
		});
		const parsed = parseCapsuleNote(note);
		assert.equal(parsed.daemonFields.payload_cid, "bafy123");
		assert.equal(parsed.daemonFields.queries_served, 42);
	});

	it("throws FrontmatterError on missing frontmatter", () => {
		assert.throws(
			() => parseCapsuleNote("just a plain markdown file"),
			FrontmatterError,
		);
	});

	it("throws FrontmatterError on invalid capsule_id", () => {
		const bad = [
			"---",
			"capsule_id: nope_no_prefix",
			"schema: capsule://x",
			"status: draft",
			"floor_price: 0.01",
			"computation_classes: [A]",
			"tags: []",
			"---",
			"",
		].join("\n");
		assert.throws(() => parseCapsuleNote(bad), FrontmatterError);
	});

	it("throws FrontmatterError on unknown status", () => {
		const bad = [
			"---",
			"capsule_id: cap_abc123",
			"schema: capsule://x",
			"status: maybe",
			"floor_price: 0.01",
			"computation_classes: [A]",
			"tags: []",
			"---",
			"",
		].join("\n");
		assert.throws(() => parseCapsuleNote(bad), FrontmatterError);
	});

	it("throws FrontmatterError on invalid computation_class", () => {
		const bad = [
			"---",
			"capsule_id: cap_abc123",
			"schema: capsule://x",
			"status: draft",
			"floor_price: 0.01",
			"computation_classes: [Z]",
			"tags: []",
			"---",
			"",
		].join("\n");
		assert.throws(() => parseCapsuleNote(bad), FrontmatterError);
	});

	it("preserves body after frontmatter", () => {
		const note = buildCapsuleNote({
			manifest: sampleManifest(),
			body: "# Custom body\n\nCustom content here.\n",
		});
		const parsed = parseCapsuleNote(note);
		assert.ok(parsed.body.includes("Custom body"));
		assert.ok(parsed.body.includes("Custom content here."));
	});
});

describe("replaceDaemonZone", () => {
	it("rewrites daemon fields without touching user zone or body", () => {
		const original = buildCapsuleNote({
			manifest: sampleManifest(),
			body: "# Preserved body\n\nUser-written prose.\n",
		});
		const updated = replaceDaemonZone(original, {
			payload_cid: "bafy-new",
			earnings_total: "99.99 USDC",
			queries_served: 7,
			last_accessed: "2026-04-21T00:00:00Z",
		});

		const parsed = parseCapsuleNote(updated);
		// User zone untouched. buildCapsuleNote emits defaults for sources +
		// extraction; the round-trip includes those alongside sampleManifest.
		assert.deepEqual(parsed.manifest, {
			...sampleManifest(),
			sources: [],
			extraction: "none",
		});
		// Daemon zone reflects the rewrite.
		assert.equal(parsed.daemonFields.payload_cid, "bafy-new");
		assert.equal(parsed.daemonFields.queries_served, 7);
		// Body bytes preserved.
		assert.ok(parsed.body.includes("Preserved body"));
		assert.ok(parsed.body.includes("User-written prose."));
	});

	it("throws when given a note with no frontmatter", () => {
		assert.throws(
			() => replaceDaemonZone("plain text", {}),
			FrontmatterError,
		);
	});
});
