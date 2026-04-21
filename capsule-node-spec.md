  
**THE CAPSULE PROTOCOL**

────────

**CAPSULE NODE**

Obsidian Plugin & Companion Daemon

Technical Architecture & API Specification

*Companion Document to The Capsule Protocol Manifesto*

**Version 0.1 — March 2026 — DRAFT**

**SECTION 1 — ARCHITECTURE OVERVIEW**

# **1\. System Architecture**

The Capsule Node is a two-process system: an Obsidian community plugin that provides the human interface, and a companion daemon that provides the machine interface. They share the Obsidian vault’s filesystem as their communication channel.

*Principle: Obsidian is the management layer. The daemon is the serving layer. The vault filesystem is the shared state. Neither component depends on the other to function in its primary role.*

## **1.1 Component Separation**

The two-process architecture exists for three specific reasons:

**Performance isolation.** FHE computations are CPU-intensive. Running them in Obsidian’s Electron process would freeze the UI. The daemon runs computations in isolated threads without affecting the note-taking experience.

**Availability independence.** The daemon can run as a system service, starting on boot and serving capsules 24/7 even when Obsidian is closed. This enables Tier 1 (always-on) availability without keeping a note-taking app running permanently.

**Security boundary.** The daemon handles cryptographic operations and wallet management in a separate memory space. A vulnerability in an Obsidian plugin cannot directly access the daemon’s key material.

## **1.2 Communication Model**

The plugin and daemon communicate through two channels:

* **Filesystem (primary):** The vault’s .capsule/ directory is the shared state store. The plugin writes capsule configurations and policy files; the daemon reads them. The daemon writes earnings logs and activity records; the plugin reads them. Both watch for filesystem changes via OS-native file watchers (inotify on Linux, FSEvents on macOS, ReadDirectoryChangesW on Windows).

* **Localhost HTTP API (secondary):** The daemon exposes a REST API on 127.0.0.1:{configured\_port} for real-time operations that the filesystem cannot handle efficiently: starting/stopping the node, querying live status, triggering manual manifest refresh, viewing active WebSocket connections, and approving/rejecting bids.

The filesystem channel handles all persistent state. The localhost API handles all ephemeral operations. This means the plugin can be uninstalled and reinstalled without losing any data, and the daemon can be restarted without the plugin being aware.

## **1.3 High-Level Data Flow**

The complete flow from vault to agent transaction:

1. User creates or imports a capsule note in Obsidian with the required YAML frontmatter.

2. The plugin validates the frontmatter, generates a capsule ID, and writes an encrypted payload to .capsule/payloads/{capsule\_id}.enc using the owner’s derived key.

3. The plugin writes a manifest file to .capsule/manifests/{capsule\_id}.json.

4. The daemon detects the new manifest via filesystem watcher.

5. The daemon parses the manifest, registers it with the decentralized index, and opens an HTTP endpoint for the capsule.

6. An agent discovers the capsule in the index, hits the endpoint, receives HTTP 402\.

7. The agent pays via x402. The daemon verifies payment, evaluates the policy, and dispatches the FHE computation.

8. The daemon writes a transaction record to .capsule/activity/{capsule\_id}.jsonl.

9. The daemon updates the manifest with incremented query count and earnings total.

10. The plugin detects the manifest change, updates the YAML frontmatter in the corresponding Obsidian note.

11. The user sees updated earnings in their capsule note and dashboard.

**SECTION 2 — VAULT STRUCTURE**

# **2\. Vault Filesystem Layout**

The Capsule Node uses a defined directory structure within the Obsidian vault. User-facing notes live in the vault root (organized however the user prefers). Machine-managed state lives in the .capsule/ hidden directory.

| vault-root/ ├── .capsule/                              \# Machine-managed (hidden from Obsidian) │   ├── config.yaml                      \# Node configuration │   ├── identity/ │   │   ├── did.json                     \# Owner DID document │   │   ├── keyring.enc                  \# Encrypted key material │   │   └── credentials/                 \# Verifiable Credentials │   │       ├── vc\_ontario\_health.json │   │       └── vc\_td\_bank\_account.json │   ├── payloads/                        \# Encrypted capsule data │   │   ├── cap\_8f3a2b.enc               \# Binary encrypted payload │   │   └── cap\_c91d4e.enc │   ├── manifests/                       \# Machine-readable manifest JSONs │   │   ├── cap\_8f3a2b.json │   │   └── cap\_c91d4e.json │   ├── policies/                        \# Compiled access policy files │   │   ├── default.json │   │   └── health\_research\_only.json │   ├── activity/                        \# Transaction logs │   │   ├── cap\_8f3a2b.jsonl             \# One JSON object per line │   │   └── earnings\_summary.json        \# Aggregated earnings │   ├── bounties/                        \# Active bounty participation │   │   └── bounty\_a3f8.json │   ├── pods/                            \# Pod membership config │   │   └── my\_pod.json │   ├── runtime/                         \# FHE WASM binaries │   │   ├── tfhe\_bg.wasm │   │   └── computation\_circuits/ │   └── daemon.pid                       \# Daemon process ID (if running) │ ├── 🏥 Health/                           \# User-organized capsule notes │   ├── Glucose \- Continuous.md │   └── Sleep & HRV.md ├── 💰 Finance/ │   └── Banking \- Transactions.md ├── 📊 Dashboard/ │   ├── Earnings Overview.md │   ├── Active Bounties.md │   └── Network Status.md └── ⚙️ Policies/     ├── Default Access Policy.md     └── Health \- Research Only.md |
| :---- |

## **2.1 Config File**

The .capsule/config.yaml file contains all node configuration:

| \# .capsule/config.yaml   node:   version: "0.1.0"   did: "did:key:z6Mkf5rGMoatrSj1f..."   display\_name: "anon-capsule-node"        \# never reveals real name   daemon:   port: 7402                                \# localhost API port   external\_port: 8402                       \# public-facing capsule endpoint port   auto\_start: true                          \# start daemon on system boot   max\_concurrent\_computations: 4   computation\_timeout\_seconds: 120   log\_level: "info"   wallet:   type: "embedded"                          \# "embedded" | "external"   network: "base"                           \# "base" | "solana" | "base,solana"   address: "0x7f3a..."                      \# USDC receiving address   auto\_withdraw\_threshold: 50.00            \# auto-withdraw to cold wallet at $50   cold\_wallet: "0x9b2c..."   availability:   tier: 1                                   \# 1=always-on, 2=coop, 3=relay, 4=intermittent   relay\_url: null                           \# relay endpoint if tier 3   pod\_id: null                              \# pod identifier if tier 2   heartbeat\_interval\_seconds: 30   index:   provider: "thegraph"                      \# "thegraph" | "origintrail" | "custom"   endpoint: "https://index.capsuleprotocol.network"   refresh\_interval\_seconds: 300   storage:   payload\_location: "local"                 \# "local" | "ipfs" | "arweave" | "s3"   ipfs\_gateway: "https://w3s.link"   pin\_to\_filecoin: false   security:   key\_derivation: "bip39"   threshold\_scheme: "2-of-3"   auto\_lock\_minutes: 30                     \# lock keyring after inactivity   allowed\_origins: \["\*"\]                    \# CORS for external agent access |
| :---- |

## **2.2 Capsule Note Frontmatter Specification**

Every capsule note in the vault must include YAML frontmatter conforming to this schema. The plugin validates frontmatter on save and highlights errors inline.

| \--- \# ═══ Required Fields ═══ capsule\_id: "cap\_8f3a2b"                  \# Generated by plugin, never edited manually schema: "capsule://health.glucose.continuous" status: active                              \# active | paused | draft | archived   \# ═══ Pricing ═══ floor\_price: "0.08 USDC/query" computation\_classes: \[A, B\]                 \# A=simple, B=analytical, C=complex   \# ═══ Availability ═══ availability: tier-1                        \# inherits from config unless overridden   \# ═══ Data Profile ═══ temporal\_range: \[2023-01-15, 2026-03-21\] record\_count: 315000 completeness: 0.94                          \# 0.0 \- 1.0 freshness: 2026-03-20                       \# last data update   \# ═══ Credentials & Provenance ═══ credentials:   \- issuer: "did:web:ontariohealth.ca"     type: "VerifiedPatientData"     issued: 2024-06-15 tags: \[glucose, cgm, diabetes-research, longitudinal\] geo: CA                                     \# ISO 3166-1 alpha-2   \# ═══ Policy ═══ policy: "\[\[Health \- Research Only\]\]"        \# Wikilink to policy note   \# ═══ Computed Fields (daemon-managed, read-only) ═══ payload\_cid: "bafy2bzace..." storage: local earnings\_total: 14.23 queries\_served: 847 last\_accessed: 2026-03-20T14:32:00Z reputation: 0.92 \--- |
| :---- |

The plugin enforces a strict boundary between user-editable fields (everything above the “Computed Fields” comment) and daemon-managed fields (below the comment). Users can freely edit pricing, status, tags, and policy links. The daemon updates earnings, query counts, and reputation.

## **2.3 Policy Note Format**

Access policies are written as human-readable Obsidian notes with a structured YAML frontmatter that compiles to a machine-readable policy. This is a key UX innovation: policies are documents you can read, link, and reason about — not opaque JSON blobs.

| \--- policy\_id: "pol\_health\_research" type: abe                                   \# abe | threshold | simple description: "Research-only access for health capsules"   rules:   \- credential: "ResearchInstitution"     condition: "purpose \== 'research'"     allowed\_computations: \[A, B\]     max\_queries\_per\_day: 100     expires: null                            \# null \= no expiry     \- credential: "LicensedPhysician"     condition: "affiliation contains 'ontario'"     allowed\_computations: \[A, B, C\]     max\_queries\_per\_day: 50     expires: null   blocked:   \- "did:key:z6Mkblacklisted..."           \# specific blocked DIDs   royalty:   percentage: 0.05                          \# 5% on derived value   recipient: owner                          \# "owner" or a specific DID/address \---   \# Health \- Research Only   This policy allows access to health capsules for \*\*research purposes\*\* from verified research institutions and licensed physicians.   \#\# Who Can Access \- Research institutions with a valid \`ResearchInstitution\` VC \- Licensed physicians affiliated with Ontario health networks   \#\# Restrictions \- Maximum 100 queries per day per agent \- No commercial use without separate negotiation \- 5% royalty on any derived insights   \#\# Applied To \- \[\[Glucose \- Continuous\]\] \- \[\[Sleep & HRV\]\] \- \[\[Cardiology \- ECG History\]\] |
| :---- |

When the plugin detects a policy note (identified by policy\_id in frontmatter), it compiles the YAML rules into a machine-readable JSON policy and writes it to .capsule/policies/. The daemon reads these compiled policies for runtime enforcement.

**SECTION 3 — THE OBSIDIAN PLUGIN**

# **3\. Plugin Architecture**

The capsule-node Obsidian plugin (community plugin, distributed via Obsidian’s plugin registry) is a TypeScript plugin that provides the human-facing interface for managing capsules, monitoring earnings, and controlling the daemon.

## **3.1 Plugin Modules**

| Module | Responsibility | Category |
| :---- | :---- | :---- |
| **CapsuleManager** | Validates and manages capsule note frontmatter. Generates capsule IDs. Handles data import (CSV, JSON, FHIR bundles, bank exports). Encrypts payloads and writes to .capsule/payloads/. Creates manifest JSONs in .capsule/manifests/. | Core |
| **PolicyCompiler** | Parses policy note YAML, validates rule syntax, compiles to machine-readable JSON. Writes compiled policies to .capsule/policies/. Validates capsule-to-policy links. | Core |
| **DaemonBridge** | Communicates with the companion daemon via localhost HTTP. Handles daemon lifecycle: start, stop, restart, status check. Provides real-time status updates to the plugin UI. | Core |
| **DashboardRenderer** | Renders dashboard notes with live data from .capsule/activity/ and the daemon API. Provides earnings charts, query volume graphs, agent activity timelines, and network status panels. Uses Obsidian’s MarkdownPostProcessor API. | UI |
| **CapsuleDecorator** | Adds visual indicators to capsule notes: status badges (active/paused/draft), availability dots (green/yellow/gray), earnings counters, and last-accessed timestamps. Uses Obsidian’s EditorExtension API. | UI |
| **ImportWizard** | Guided workflow for creating capsules from raw data. Supports CSV import with column mapping, JSON/FHIR bundle parsing, bank statement parsing (OFX/QFX), and health device exports (Apple Health XML, Dexcom CSV, Fitbit JSON). | UX |
| **BountyBrowser** | Displays open bounties from the decentralized index. Filters by matching capsule types the user owns. Shows bounty terms, remaining capacity, and estimated earnings. Enables one-click opt-in. | UX |
| **SettingsPanel** | Configuration UI for .capsule/config.yaml. Wallet setup, network selection, availability tier configuration, relay/pod settings, and security preferences. | Config |

## **3.2 Plugin Lifecycle Hooks**

The plugin responds to the following Obsidian lifecycle events:

* **onload():** Initialize the DaemonBridge. Start filesystem watchers on .capsule/. Register Markdown post-processors for dashboard rendering. Register editor extensions for capsule decoration. Check if daemon is running; if auto\_start is true and daemon is not running, launch it.

* **onunload():** Stop filesystem watchers. Do NOT stop the daemon (it should continue running independently). Clean up UI decorators.

* **onFileCreate():** If a new .md file is created with capsule frontmatter, validate it, generate capsule\_id if missing, encrypt and store payload, write manifest.

* **onFileModify():** If a capsule note’s user-editable frontmatter changes (price, status, policy, tags), recompile the manifest and write the update. If a policy note changes, recompile the policy JSON.

* **onFileDelete():** If a capsule note is deleted, prompt the user: archive the capsule (keep payload, remove from index) or permanently delete (destroy payload, remove manifest).

## **3.3 Data Import Pipeline**

The ImportWizard module provides a structured pipeline for converting raw data exports into capsules:

| Step | Name | Description |
| :---- | :---- | :---- |
| 1 | **Source Selection** | User selects data source type (health device, bank, streaming service, manual CSV) and uploads or points to the raw file. |
| 2 | **Schema Matching** | Plugin queries the schema registry to find matching capsule types. Suggests the best match based on file structure and column names. User confirms or selects manually. |
| 3 | **Field Mapping** | Interactive column/field mapper. Plugin auto-maps recognized fields; user maps remaining fields manually. Validates data types and ranges against schema constraints. |
| 4 | **Quality Assessment** | Plugin scans the data for completeness, temporal gaps, outliers, and consistency. Generates a completeness score and quality report. Flags issues for user review. |
| 5 | **Policy Assignment** | User selects an existing policy note or creates a new one. Sets floor price and computation classes. The plugin suggests pricing based on market data from the index. |
| 6 | **Encryption & Storage** | Plugin encrypts the mapped data using the owner’s derived key, writes the encrypted payload to .capsule/payloads/, generates the CID, and creates the manifest JSON. |
| 7 | **Note Generation** | Plugin generates the capsule note with populated frontmatter, a human-readable data profile, and wikilinks to the assigned policy. User can customize the note body. |
| 8 | **Publication** | Manifest is pushed to the daemon, which registers it with the decentralized index. Capsule goes live (or stays in draft status if the user chose that). |

## **3.4 Dashboard System**

Dashboard notes use Obsidian’s MarkdownPostProcessor API to render live data inline. The plugin recognizes fenced code blocks with the language identifier capsule-dashboard and replaces them with rendered HTML views.

| \# Earnings Overview   \`\`\`capsule-dashboard type: earnings-summary period: this-month compare: last-month chart: bar \`\`\`   \#\# Top Earning Capsules   \`\`\`capsule-dashboard type: capsule-ranking sort: earnings-desc limit: 10 \`\`\`   \#\# Recent Agent Activity   \`\`\`capsule-dashboard type: activity-feed limit: 20 show: \[timestamp, agent\_did, capsule, computation, payment\] \`\`\`   \#\# Bounty Opportunities   \`\`\`capsule-dashboard type: matching-bounties sort: payout-desc filter: my-capsule-types \`\`\` |
| :---- |

The plugin fetches data from two sources: the .capsule/activity/ directory for historical data, and the daemon’s localhost API for real-time data (active connections, pending bids, live bounties). The rendered views update on a configurable interval (default: 30 seconds).

**SECTION 4 — THE COMPANION DAEMON**

# **4\. Daemon Architecture**

The capsule-daemon (capsuled) is a lightweight, high-performance system service written in Rust. It handles all machine-facing operations: serving capsule endpoints, processing x402 payments, executing FHE computations, managing registry presence, and maintaining the activity log.

## **4.1 Why Rust**

* Performance: FHE computations are CPU-bound. Rust’s zero-cost abstractions and lack of garbage collection give maximum throughput.

* Memory safety: The daemon handles cryptographic key material. Memory safety guarantees prevent key leakage through buffer overflows or use-after-free bugs.

* Small binary: The daemon distributes as a single static binary (\~15–25MB). No runtime dependencies, no JVM, no Node.js.

* Cross-platform: Compiles to macOS (ARM and x86), Windows, and Linux from a single codebase.

* System service integration: Native support for systemd (Linux), launchd (macOS), and Windows Service APIs.

## **4.2 Daemon Process Model**

The daemon runs as a multi-threaded process with the following thread architecture:

| Thread Pool | Count | Responsibility |
| :---- | :---- | :---- |
| **Main / HTTP** | 1 (async, tokio) | Accepts inbound HTTP connections. Handles x402 negotiation. Routes requests to computation workers. Serves the localhost management API. |
| **Computation Workers** | Configurable (default: 4\) | Execute FHE computations on capsule payloads. Each worker loads the TFHE runtime and processes one computation at a time. Isolated from each other and from the HTTP thread. |
| **Filesystem Watcher** | 1 | Monitors .capsule/ for changes to manifests, policies, and config. Triggers manifest re-registration, policy recompilation, or daemon reconfiguration. |
| **Registry Client** | 1 | Manages index registration, heartbeat, deregistration. Syncs bounty lists. Publishes manifest updates. Handles The Graph or OriginTrail indexer communication. |
| **Payment Settler** | 1 | Verifies x402 payment payloads. Interacts with the facilitator for on-chain settlement. Manages the embedded wallet. Handles auto-withdrawal to cold storage. |

## **4.3 Request Processing Pipeline**

When an agent hits a capsule endpoint, the daemon processes the request through a defined pipeline:

| \# | Stage | Description |
| :---- | :---- | :---- |
| 1 | **Parse** | Extract capsule CID, computation type, parameters, and payment header from the HTTP request. Validate request structure. |
| 2 | **Manifest Lookup** | Find the capsule manifest in the local manifest store. If the capsule doesn’t exist or is paused/archived, return 404\. |
| 3 | **Payment Check** | If no X-PAYMENT header: return HTTP 402 with PAYMENT-REQUIRED header containing price, wallet address, accepted networks, and expiry. Pipeline ends here for unpaid requests. |
| 4 | **Payment Verify** | Validate the payment payload (signature, amount, recipient, expiry). Submit to x402 facilitator for on-chain verification. If invalid, return 402 with error details. |
| 5 | **Policy Evaluate** | Load the compiled policy for this capsule. Verify the agent’s DID meets the credential requirements. Check rate limits (queries per day). Verify the computation type is allowed. If denied, return 403 with reason. |
| 6 | **Computation Dispatch** | Queue the computation on a worker thread. Load the encrypted payload from .capsule/payloads/. Load the appropriate FHE circuit for the computation type. Execute the computation. |
| 7 | **Result Return** | Return the encrypted computation result as the HTTP response body (Content-Type: application/octet-stream). Include X-CAPSULE-PROVENANCE header with computation receipt hash. |
| 8 | **Audit Log** | Append a transaction record to .capsule/activity/{capsule\_id}.jsonl. Update the manifest with incremented query count and earnings total. |

## **4.4 The FHE Runtime**

The daemon embeds a TFHE computation engine compiled from Rust-native TFHE-rs. Computation circuits are pre-compiled for each computation class and capsule schema type.

**Circuit Registry**

Each schema type defines a set of valid computation circuits in the on-chain schema registry. The daemon downloads and caches compiled circuits for schemas it serves. The circuit registry is organized by schema URI and computation class:

| .capsule/runtime/computation\_circuits/ ├── health.glucose.continuous/ │   ├── A\_statistical\_summary.circuit   \# Mean, median, std dev, min, max │   ├── A\_range\_check.circuit            \# Is value within threshold X-Y? │   ├── A\_time\_in\_range.circuit          \# % time glucose within target range │   ├── B\_trend\_analysis.circuit         \# 30/60/90 day trend vectors │   └── B\_correlation.circuit            \# Correlation with meal/exercise events ├── finance.transactions.banking/ │   ├── A\_balance\_summary.circuit │   ├── A\_category\_totals.circuit │   └── B\_spending\_pattern.circuit └── manifest.json                        \# Circuit versions and checksums |
| :---- |

Circuits are versioned and integrity-checked via SHA-256 hashes published in the schema registry. The daemon refuses to execute a circuit whose hash does not match the registry entry, preventing tampered computation.

**SECTION 5 — DAEMON API SPECIFICATION**

# **5\. Public API (Agent-Facing)**

The public API is exposed on the configured external\_port (default 8402). It is the interface that AI agents interact with. All endpoints implement the x402 payment protocol where applicable.

**Base URL**

https://{node\_address}:{external\_port}/v1

| GET | /capsules | List all active capsule manifests (paginated). No payment required. Returns array of manifest summaries. |
| :---: | :---- | :---- |

| GET | /capsules/{cid}/manifest | Full manifest for a specific capsule. No payment required. Returns complete manifest JSON. |
| :---: | :---- | :---- |

| GET | /capsules/{cid}/compute | Execute computation. Requires x402 payment. Query params: type (computation URI), params (base64-encoded). Returns encrypted result. |
| :---: | :---- | :---- |

| POST | /capsules/{cid}/bid | Submit a bid for computation below floor price. Body: { offered\_price, computation\_type, expiry }. Returns 202 (pending) or 406 (rejected). |
| :---: | :---- | :---- |

| POST | /capsules/{cid}/bounty/{bid}/opt-in | Owner opts capsule into an open bounty. Returns escrow contract details. |
| :---: | :---- | :---- |

| GET | /node/info | Public node information: DID, availability tier, uptime score, total capsule count, supported schemas. No payment. |
| :---: | :---- | :---- |

| GET | /schemas | List capsule schemas this node serves. No payment. |
| :---: | :---- | :---- |

| WS | /events | WebSocket stream of real-time node events (new capsules, status changes). Agents subscribe for live updates. |
| :---: | :---- | :---- |

**x402 Response Format**

When a computation endpoint is hit without payment, the daemon returns:

| HTTP/1.1 402 Payment Required Content-Type: application/json PAYMENT-REQUIRED: \<base64-encoded PaymentRequired object\>   {   "capsule\_cid": "cap\_8f3a2b",   "computation\_type": "A\_statistical\_summary",   "price": {     "amount": "0.08",     "currency": "USDC",     "network": "eip155:8453"                 // Base   },   "recipient": "0x7f3a...",   "expiry": "2026-03-21T15:00:00Z",   "supported\_schemes": \["exact"\],   "capsule\_quality": {     "completeness": 0.94,     "temporal\_range": \["2023-01-15", "2026-03-21"\],     "credentials": \["VerifiedPatientData"\],     "reputation": 0.92   } } |
| :---- |

# **6\. Management API (Plugin-Facing)**

The management API is exposed on 127.0.0.1:{daemon\_port} (default 7402). It is only accessible from the local machine and provides the interface the Obsidian plugin uses for real-time operations.

**Base URL**

http://127.0.0.1:7402/api/v1

**Node Lifecycle**

| GET | /status | Daemon status: running, uptime, active connections, memory usage, computation queue depth, wallet balance. |
| :---: | :---- | :---- |

| POST | /start | Start serving capsules (publish manifests, open external port, begin heartbeat). |
| :---: | :---- | :---- |

| POST | /stop | Stop serving (deregister from index, close external port, stop heartbeat). Daemon process continues running. |
| :---: | :---- | :---- |

| POST | /restart | Full restart: reload config, re-read all manifests, re-register. |
| :---: | :---- | :---- |

**Capsule Management**

| GET | /capsules | List all capsules with live status (active connections, earnings, queue depth per capsule). |
| :---: | :---- | :---- |

| GET | /capsules/{cid}/activity | Recent activity for a capsule. Query params: limit, since. Returns array of transaction records. |
| :---: | :---- | :---- |

| POST | /capsules/{cid}/pause | Pause a capsule (stop serving but keep in registry as paused). Agents see "temporarily unavailable". |
| :---: | :---- | :---- |

| POST | /capsules/{cid}/resume | Resume a paused capsule. |
| :---: | :---- | :---- |

| POST | /capsules/{cid}/refresh | Force re-read manifest from filesystem and re-register with index. |
| :---: | :---- | :---- |

| DELETE | /capsules/{cid} | Remove capsule from index. Does not delete the payload file (that’s the plugin’s responsibility). |
| :---: | :---- | :---- |

**Marketplace**

| GET | /bounties | List open bounties matching this node’s capsule types. Filtered by schema match and minimum payout. |
| :---: | :---- | :---- |

| GET | /bounties/{bid} | Full bounty details including escrow status, fill progress, and terms. |
| :---: | :---- | :---- |

| GET | /bids/pending | List pending bids awaiting owner approval. Each bid includes agent DID, offered price, computation type. |
| :---: | :---- | :---- |

| POST | /bids/{bid\_id}/approve | Approve a pending bid. Triggers x402 settlement and computation execution. |
| :---: | :---- | :---- |

| POST | /bids/{bid\_id}/reject | Reject a pending bid with optional reason. |
| :---: | :---- | :---- |

| GET | /market/pricing | Market pricing data for schemas this node serves. Includes averages, percentiles, and demand indicators. |
| :---: | :---- | :---- |

**Wallet & Earnings**

| GET | /wallet/balance | Current wallet balance (USDC) across configured networks. |
| :---: | :---- | :---- |

| GET | /wallet/transactions | Recent payment transactions. Query params: limit, since, capsule\_cid. |
| :---: | :---- | :---- |

| POST | /wallet/withdraw | Manual withdrawal to cold wallet. Body: { amount, destination }. |
| :---: | :---- | :---- |

| GET | /earnings/summary | Aggregated earnings by period (day, week, month), by capsule, by schema type. |
| :---: | :---- | :---- |

| GET | /earnings/projection | Projected monthly earnings based on current query velocity and pricing. |
| :---: | :---- | :---- |

**Network & Registry**

| GET | /network/peers | Connected peers (if in a pod). Pod health, shard distribution, collective uptime. |
| :---: | :---- | :---- |

| GET | /network/index-status | Registry synchronization status. Last heartbeat, manifest count, index health. |
| :---: | :---- | :---- |

| POST | /network/reindex | Force re-publish all manifests to the decentralized index. |
| :---: | :---- | :---- |

| GET | /network/relay | Relay connection status (if tier 3). Relay latency, fee, and throughput. |
| :---: | :---- | :---- |

**SECTION 6 — DATA FORMATS & EVENTS**

# **7\. Activity Log Format**

The daemon writes transaction records as newline-delimited JSON (JSONL) files in .capsule/activity/. Each capsule has its own log file, plus a global log for cross-capsule analytics.

| // .capsule/activity/cap\_8f3a2b.jsonl // One JSON object per line, appended chronologically   {   "event\_id": "evt\_f8a3b2c1",   "timestamp": "2026-03-20T14:32:00.847Z",   "type": "computation",   "capsule\_cid": "cap\_8f3a2b",   "agent\_did": "did:key:z6MkpharmaResearchBot...",   "computation": {     "type": "A\_statistical\_summary",     "class": "A",     "duration\_ms": 342,     "result\_size\_bytes": 1024   },   "payment": {     "amount": "0.08",     "currency": "USDC",     "network": "base",     "tx\_hash": "0xabc123...",     "facilitator": "coinbase"   },   "policy\_applied": "pol\_health\_research",   "provenance\_hash": "sha256:def456..." } |
| :---- |

# **8\. Earnings Summary Format**

The daemon maintains a rolling earnings summary in .capsule/activity/earnings\_summary.json, updated after every transaction. The plugin reads this file for dashboard rendering.

| {   "generated\_at": "2026-03-21T00:00:00Z",   "totals": {     "all\_time": { "earnings": "142.37", "queries": 8473 },     "this\_month": { "earnings": "23.80", "queries": 1247 },     "last\_month": { "earnings": "19.45", "queries": 1103 },     "today": { "earnings": "1.12", "queries": 58 }   },   "by\_capsule": {     "cap\_8f3a2b": {       "schema": "capsule://health.glucose.continuous",       "earnings\_this\_month": "14.23",       "queries\_this\_month": 847,       "avg\_price": "0.0168",       "unique\_agents": 12     }   },   "by\_schema": {     "health.glucose.continuous": { "earnings": "14.23", "queries": 847 },     "finance.transactions.banking": { "earnings": "9.57", "queries": 400 }   },   "top\_agents": \[     { "did": "did:key:z6Mk...", "total\_paid": "8.40", "queries": 105 }   \] } |
| :---- |

**SECTION 7 — SECURITY ARCHITECTURE**

# **9\. Security Model**

## **9.1 Key Material Protection**

The .capsule/identity/keyring.enc file contains all cryptographic key material, encrypted at rest with a passphrase-derived key (Argon2id KDF). The daemon decrypts the keyring into memory on startup (requiring passphrase entry or hardware token authentication) and holds derived keys in memory for the session.

Key material in memory is protected through:

* mlock() / VirtualLock() to prevent key material from being swapped to disk.

* Zeroization on process exit using the zeroize crate (Rust).

* Auto-lock after configured inactivity period (default: 30 minutes). After auto-lock, the daemon continues running but refuses computation requests until re-authenticated.

## **9.2 Transport Security**

The public-facing API (external\_port) must use TLS. The daemon supports two TLS modes:

* **Auto-TLS:** The daemon provisions a TLS certificate via Let’s Encrypt ACME (requires a domain name pointing to the node). This is the recommended mode for Tier 1 nodes.

* **Self-signed:** For local network or development use. Agents must explicitly trust the certificate.

The management API (daemon\_port) listens only on 127.0.0.1 and does not require TLS (localhost traffic does not traverse the network).

## **9.3 Rate Limiting & Abuse Prevention**

The daemon enforces multiple rate limiting layers:

* Per-agent rate limits: configurable max queries per agent DID per day, enforced per policy.

* Global rate limits: maximum inbound requests per second across all capsules (protects against DDoS).

* Computation queue depth: maximum pending computations (default: 20). New requests receive 503 when the queue is full.

* Payment validation timeout: x402 payment must be verified within 10 seconds or the request is dropped.

* Agent reputation filtering: the daemon can optionally query the index for agent reputation scores and refuse agents below a threshold.

## **9.4 Vault Security**

The .capsule/ directory permissions:

* On Unix: directory permissions set to 700 (owner read/write/execute only). Payload files set to 600\.

* On Windows: NTFS ACL restricts to the current user only.

* The daemon runs as the same user who owns the vault. It does not require root/admin privileges.

* If the vault is on an encrypted filesystem (FileVault, LUKS, BitLocker), the capsule payloads benefit from double encryption (filesystem-level \+ capsule-level).

**SECTION 8 — INSTALLATION & DEPLOYMENT**

# **10\. Installation**

## **10.1 Plugin Installation**

The capsule-node plugin is installed through Obsidian’s community plugin browser:

12. Open Obsidian Settings and navigate to Community Plugins.

13. Search for "Capsule Node" and click Install.

14. Enable the plugin. On first activation, the plugin runs the setup wizard.

The setup wizard walks the user through:

* Creating or importing a DID (generate new, or import from an existing wallet).

* Setting up the wallet (embedded or connect external wallet via WalletConnect).

* Choosing an availability tier and configuring network settings.

* Downloading the companion daemon (platform-specific binary).

* Initializing the .capsule/ directory structure.

## **10.2 Daemon Installation**

The daemon binary is distributed alongside the plugin and installed automatically during setup. Manual installation is available for advanced users:

| \# macOS (ARM) curl \-L https://releases.capsuleprotocol.network/latest/capsuled-darwin-arm64 \-o capsuled chmod \+x capsuled sudo mv capsuled /usr/local/bin/   \# Linux (x86\_64) curl \-L https://releases.capsuleprotocol.network/latest/capsuled-linux-amd64 \-o capsuled chmod \+x capsuled sudo mv capsuled /usr/local/bin/   \# Windows (PowerShell) Invoke-WebRequest https://releases.capsuleprotocol.network/latest/capsuled-win-x64.exe \-O capsuled.exe   \# Start with vault path capsuled \--vault /path/to/obsidian/vault   \# Install as system service (auto-start on boot) capsuled install \--vault /path/to/obsidian/vault |
| :---- |

## **10.3 Deployment Topologies**

The system supports four deployment models matching the availability tiers:

| Topology | Setup | Characteristics |
| :---- | :---- | :---- |
| **Desktop Direct** | Obsidian \+ daemon on primary computer. Daemon runs as a system service. Port forwarding or Cloudflare Tunnel for external access. | Tier 1-4 depending on uptime. Simplest setup. Best for getting started. Requires port accessibility. |
| **Dedicated Device** | Daemon only on a Raspberry Pi, NUC, or old laptop. Vault synced from primary machine via Syncthing or git. Headless operation. | Tier 1 (always-on). Low power consumption ($5–10/year electricity). No Obsidian needed on the device; daemon reads vault directly. |
| **VPS Hosted** | Daemon on a cheap VPS ($5–10/month). Vault synced from local machine. Plugin connects to remote daemon via SSH tunnel or authenticated API. | Tier 1 with high availability. Better bandwidth. Suitable for users with many capsules or high query volume. |
| **Pod Cooperative** | Daemon on each pod member’s machine. Pod coordination via the pod protocol. Threshold-sharded capsules distributed across members. | Tier 2\. Collective availability. No single member needs to be always-on. Requires pod formation and trust. |

**SECTION 9 — DEVELOPMENT ROADMAP**

# **11\. Build Sequence**

The plugin and daemon are developed in parallel tracks with integration milestones.

## **Phase 1: Foundation (Weeks 1–6)**

**Daemon Track**

* Scaffold Rust project with Tokio async runtime, Axum HTTP framework.

* Implement filesystem watcher for .capsule/ directory.

* Implement manifest parser and in-memory capsule registry.

* Build the public HTTP server with x402 stub (returns 402 with mock payment terms).

* Build the management API with /status, /capsules, /start, /stop endpoints.

* Implement basic wallet integration (USDC on Base via ethers-rs).

**Plugin Track**

* Scaffold Obsidian plugin with TypeScript.

* Implement frontmatter validation for capsule notes.

* Implement capsule ID generation and .capsule/ directory initialization.

* Build DaemonBridge module (localhost HTTP client).

* Build SettingsPanel with config.yaml editor.

* Implement basic CapsuleDecorator (status badges on capsule notes).

**Integration Milestone**

*Plugin can create a capsule note, the daemon detects it, and the capsule appears as an endpoint returning HTTP 402 to any client. Management API returns capsule status.*

## **Phase 2: Import & Payment (Weeks 7–12)**

**Daemon Track**

* Integrate x402 payment verification using @x402/core.

* Implement TFHE-rs runtime with a single computation circuit (Class A: statistical summary for glucose data).

* Build the full request processing pipeline (parse → pay → policy → compute → return → log).

* Implement activity logging (JSONL writer).

* Build registry client stub (announces to a test index).

**Plugin Track**

* Build ImportWizard for CSV import with column mapping.

* Implement payload encryption (AES-256-GCM for initial version, TFHE migration later).

* Build PolicyCompiler (YAML to JSON policy compilation).

* Build DashboardRenderer with earnings-summary and activity-feed views.

* Add Dexcom CSV and Apple Health XML importers.

**Integration Milestone**

*End-to-end flow: user imports a Dexcom CSV, plugin creates an encrypted capsule, daemon serves it, an agent pays with x402 on Base testnet, FHE computation runs, result returns, earnings update in Obsidian.*

## **Phase 3: Marketplace & Discovery (Weeks 13–18)**

**Daemon Track**

* Implement decentralized index registration (GRC-20 testnet or mock indexer).

* Build bounty protocol (monitor open bounties, opt-in flow, escrow verification).

* Implement bid management (receive, queue, accept/reject via management API).

* Add Class B computation circuits (trend analysis, correlation).

* Implement relay protocol for Tier 3 availability.

**Plugin Track**

* Build BountyBrowser with matching, filtering, and opt-in.

* Build bid approval UI in sidebar.

* Add market pricing widget to dashboard.

* Implement bank statement importers (OFX/QFX).

* Add streaming history importers (Netflix, Spotify export formats).

**Integration Milestone**

*Full marketplace loop: agents discover capsules through the index, pay for computation, submit bids, participate in bounties. Users manage everything through Obsidian notes and dashboard views.*

## **Phase 4: Pods & Scale (Weeks 19–24)**

**Daemon Track**

* Implement pod protocol (formation, shard distribution, collective serving, health monitoring).

* Implement threshold encryption for pod-sharded capsules.

* Add Class C computation circuits.

* Performance optimization: computation caching, batch settlement, connection pooling.

* Implement auto-TLS via Let’s Encrypt.

* System service installers for macOS, Linux, Windows.

**Plugin Track**

* Build pod management UI (formation wizard, member dashboard, health view).

* Implement advanced dashboard: per-agent analytics, schema-level trends, earnings projections.

* Build capsule template marketplace (community-shared import templates).

* Implement vault backup and migration tools.

**Integration Milestone**

*Pod of 10+ members serving capsules cooperatively. Tier 2 availability demonstrated. Full plugin UX polished for community release.*

# **12\. Summary of Key Decisions**

| Decision | Choice | Rationale |
| :---- | :---- | :---- |
| **Human interface** | Obsidian plugin | Existing UX, massive user base, local-first, markdown-native, graph view |
| **Machine interface** | Rust daemon (capsuled) | Performance, memory safety, small binary, cross-platform, system service support |
| **Communication channel** | Filesystem \+ localhost HTTP | Decoupled, fault-tolerant, inspectable, no IPC complexity |
| **Capsule metadata format** | YAML frontmatter in markdown | Human-readable, editable, version-controllable, Obsidian-native |
| **Policy format** | YAML in policy notes | Readable as documents, linkable, compilable to machine format |
| **Activity log format** | JSONL per capsule | Append-only, streamable, greppable, no database dependency |
| **FHE runtime** | TFHE-rs (Rust-native) | Best Rust integration, active development, lattice-based (quantum-resistant) |
| **Payment rail** | x402 with USDC on Base | Sub-cent fees, sub-second settlement, agent-native, growing ecosystem |
| **TLS** | Let’s Encrypt auto-provisioning | Zero-config for Tier 1 nodes, trusted certificates |
| **Daemon language** | Rust | Performance, safety, single-binary distribution, async ecosystem (tokio) |
| **Plugin language** | TypeScript | Obsidian’s native plugin language, ecosystem compatibility |
| **Sync mechanism** | User’s choice (Syncthing, git, Obsidian Sync) | No lock-in, existing tools work, encrypted payloads safe in transit |

*The Obsidian vault is not just a container for capsules — it is the capsule node. The markdown notes are the manifests. The wikilinks are the graph. The frontmatter is the schema. The plugin is the bridge. The daemon is the engine. And the filesystem they share is the protocol’s heartbeat.*

**END OF SPECIFICATION**

Capsule Node — v0.1 — March 2026
