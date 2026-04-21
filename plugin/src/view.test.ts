import { strict as assert } from "node:assert";
import { describe, it } from "node:test";

import { isCapsuleNotePath, statusBadge, statusFromFrontmatter } from "./view";

describe("isCapsuleNotePath", () => {
	it("accepts notes under the configured folder", () => {
		assert.equal(isCapsuleNotePath("Capsules/cap_a1.md", "Capsules"), true);
		assert.equal(isCapsuleNotePath("Capsules/sub/cap_a1.md", "Capsules"), true);
	});

	it("rejects notes outside the folder", () => {
		assert.equal(isCapsuleNotePath("Other/note.md", "Capsules"), false);
		assert.equal(isCapsuleNotePath("cap_a1.md", "Capsules"), false);
	});

	it("rejects non-markdown files even inside the folder", () => {
		assert.equal(isCapsuleNotePath("Capsules/cap_a1.json", "Capsules"), false);
	});

	it("rejects a folder prefix that doesn't end at a path boundary", () => {
		// "Capsules" must not match "CapsulesArchive/..."
		assert.equal(
			isCapsuleNotePath("CapsulesArchive/cap_a1.md", "Capsules"),
			false,
		);
	});

	it("handles a capsuleFolder with a trailing slash", () => {
		assert.equal(isCapsuleNotePath("Capsules/cap_a1.md", "Capsules/"), true);
	});
});

describe("statusFromFrontmatter", () => {
	it("returns the status for all four valid values", () => {
		assert.equal(statusFromFrontmatter({ status: "active" }), "active");
		assert.equal(statusFromFrontmatter({ status: "paused" }), "paused");
		assert.equal(statusFromFrontmatter({ status: "draft" }), "draft");
		assert.equal(statusFromFrontmatter({ status: "archived" }), "archived");
	});

	it("returns null for missing, null, or invalid frontmatter", () => {
		assert.equal(statusFromFrontmatter(null), null);
		assert.equal(statusFromFrontmatter(undefined), null);
		assert.equal(statusFromFrontmatter({}), null);
		assert.equal(statusFromFrontmatter({ status: "bogus" }), null);
		assert.equal(statusFromFrontmatter({ status: 42 }), null);
	});
});

describe("statusBadge", () => {
	it("returns a distinct css class per status", () => {
		const classes = new Set(
			(["active", "paused", "draft", "archived"] as const).map(
				(s) => statusBadge(s).cssClass,
			),
		);
		assert.equal(classes.size, 4);
	});

	it("labels match the status name", () => {
		assert.equal(statusBadge("active").label, "active");
		assert.equal(statusBadge("paused").label, "paused");
	});
});
