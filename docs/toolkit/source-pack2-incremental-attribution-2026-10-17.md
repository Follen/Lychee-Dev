# Source pack2 incremental attribution (00d product, e475 runner)

Evidence: [terminal report](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6238394989), [14-attachment index](https://github.com/Follen/Lychee-Dev/pull/14#issuecomment-6238369445). All attachments, chunk bodies and restored files independently SHA-256 verified. The ten current owners have 34463 sealed-log references, all SHA-256 verified. Four variants’ eight native receipts (seven new plus retained Go change) exactly match the report, and original monotonic native elapsed values recompute exactly. Historical receipts copied into these archives still reference 1,254 unique older files absent from the fourteen attachments; they are not relabeled verified. The four variants retain raw stderr, seven new native receipts and full Rust index parts. The current fourteen do not retain original cold/warm raw logs or new Rust Facts pack bodies. Runtime complete five-array/order equality is recorded, but independent replay of those Facts is not possible.

## Exact same-row exclusive main-thread ledger (milliseconds)

|Component|Change|Insert|Delete|Rename|
|---|---:|---:|---:|---:|
|metadata|0.5160|0.3920|0.4830|0.4160|
|cached_index|0.0000|0.0000|0.0000|0.0000|
|index_lock|0.5640|0.5750|0.7750|0.5350|
|git_tree|91.8520|85.3330|86.5340|86.5280|
|index_serialize|577.0420|532.4190|548.8070|623.2050|
|index_publish|103.4760|98.2750|90.3720|110.8160|
|manifest|10.1860|8.9480|8.3810|9.9160|
|facts_read|586.5950|605.4750|580.5550|577.2060|
|facts_parse_sequential|0.0000|0.0000|0.0000|0.0000|
|facts_serialize|1.1390|0.0420|1.0250|1.0680|
|facts_publish|2.0160|1.3520|1.7800|3.8530|
|original_publish|7.8790|6.4320|9.0650|9.4410|
|symbols|120.7960|134.3000|116.3260|116.1860|
|facts_worker_wait|2059.5890|2086.6840|2069.0850|2066.7140|
|git_blobs_other|314.3180|337.4910|314.1720|315.4370|
|profile_other|244.6620|242.5480|215.5940|254.3980|
|native_outside_profile|282.3241|236.0598|245.8367|252.4976|
|native total|4402.9541|4376.3258|4288.7907|4428.2166|

Each column sums to the original native controllable wall. facts_parse worker durations and facts_worker_consumed are overlapping diagnostic totals, excluded from this sum. Publisher stage/write/seal/rename/close are nested within facts_publish. git_blobs_other and profile_other are residuals, not measured CPU, allocator, control or I/O costs. Native outside profile includes startup/CLI/cleanup boundary work; no further attribution is available.

## Verified retained index and derived logical byte counts

|Scenario|Full index bytes|Symbol objects|Facts successful body bytes|Directory bytes|Cached groups|New pack bytes|
|---|---:|---:|---:|---:|---:|---:|
|single-content-change|82228458|299163|41987202|274420|73|73165|
|root-insert|82229034|299164|42059747|275040|74|507|
|first-delete|82214148|299110|41987202|274420|73|65694|
|cross-prefix-rename|82226619|299163|41987202|274420|73|73173|

Logical successful bytes are derived from authenticated retained locators and healthy unchanged-group identity, with cached-record counts checked against runtime counters. They are not physical filesystem/disk telemetry and exclude failed opens or OS amplification. Full source blob bytes remain 32,271,216–32,273,783 including 4 assets. Warm whole-index read is 82,228,458 bytes from the copied report; it reads no source/Facts.

## Source attribution and priority

1. The old pipeline dispatches pure parsing before the ordered cache read. Four variants consumed 3,982–3,984 worker results despite only 1–9 true cache misses. Main-thread wait is 2,059.589–2,086.684 ms. Worker cumulative 4.73–4.84 seconds is not additive. Fix admission first: miss-only bounded workers, possible-hit barrier, full ordered verification, no queued cached Facts. No speedup is claimed until matched end-to-end measurement. New admission drains at cohort boundaries; cold cross-cohort parse/publication overlap is therefore narrower and must be measured alongside incremental gains.
2. Whole-index serialization builds serde_json::Value from Index, then serializes it again. Around 80.72–80.74 MB of the 82.21–82.23 MB blob is Symbols. All symbols are rebuilt, and declaration IDs include exact commit, so blindly reusing old complete symbols would change behavior. A later isolated borrowed persistence projection can remove the full Value DOM while preserving existing canonical field order and full index identity/controls. This batch does not change it.
3. Facts decode creates a Value DOM, canonically reserializes facts for its digest, clones the DOM, then creates owned typed Facts. It costs 577–605 ms here. A later isolated typed/raw fast path would need full fallback for noncanonical bytes and duplicate-key/escape acceptance. No claim that raw-slice hashing alone is equivalent.
4. Symbol construction costs 116–134 ms; source SHA/Git transport/queue/drop/control are not separately timed. More threads or another grouping threshold does not address the measured redundant workers.

## Verification record

The cloud verification retained download, restore, current-owner seal/native-receipt checks and exact phase arithmetic separately. Report source SHA256 ae095491fd9cd6ac28bfb9e18fa6615625bf192a85920238d34f165474c15e31; detailed JSON SHA256 14df0fbbc4ad76ddb5ed7aa8aeb28d1c5a0847ff418a9c3c3577f045ce965c95; CSV SHA256 eea5f21403006a2940a648b5c1fc04605d3c9571718d0902464988fb6a57344b. The public raw attachments are linked above; absent historical files and pack bodies are explicitly excluded from verification.

The cache-aware product candidate is separate from these original observations. Its eight Rust regressions and matched end-to-end cold/incremental performance still require execution; static review is not a runtime result.
