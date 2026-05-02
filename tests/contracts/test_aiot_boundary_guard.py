from conftest import read_doc


def test_aiot_closeout_docs_lock_device_native_ownership() -> None:
    camera = read_doc("docs/aiot-camera-canary-watchlist.md")
    ingest = read_doc("docs/aiot-device-media-ingest-watchlist.md")
    retrieval = read_doc("docs/aiot-image-retrieval-canary-evidence.md")
    acceptance = read_doc("docs/aiot-tapo-native-access-acceptance.md")
    ui_index = read_doc("docs/webui/index.html")
    ui_app = read_doc("docs/webui/app.js")

    required_phrases = [
        "## Devices & AIoT Admin Summary",
        "This round is VLM-first. The device lane should make image, snapshot, still-frame, and DVR segment artifacts easy to ingest through the existing knowledge pipeline, while leaving audio transcript extraction as follow-up work.",
        "This evidence pack is VLM-first: it treats image, snapshot, still-frame, clip, and local DVR segment artifacts as the first-version input surface for HarborDesk retrieval.",
        "the Home Device Domain owns `camera.scan`, `camera.connect`, `camera.snapshot`, `camera.share_link` (`camera.live_view` stays a compatibility alias), `device.inspect`, and `device.control`",
        "snapshot stays media-only, share output stays a signed link artifact, inspect stays read-only, and control stays device-native",
        "the device lane owns snapshot and analyze-derived image evidence, while `discover`, `connect`, `inspect`, `control`, `ptz`, and `open_stream` stay runtime/control only",
        "keep stable source/annotated linkage, preserve `caption`, `derived_text`, `tags`, `labels`, and `ingest_metadata`, and keep the sidecar shape deterministic for first-class admin page consumption",
        "the device lane emits the persisted snapshot image plus sidecar as the primary evidence candidate, with an annotated image as an optional preview candidate when present",
        "the first-version VLM input surface is image, snapshot, still-frame, clip, and DVR segment content with keyframe sidecars.",
        "discover`, `snapshot`, `share_link`, `inspect`, and `control` stay owned by the Home Device Domain",
        "`camera.share_link` remains the canonical device-lane action; `camera.live_view` stays a compatibility alias only",
        "`control` stays device-native and is not claimed by HarborOS executors or HarborOS system control",
        "retrieval evidence stays separate from runtime control",
        "HarborOS does not own device control",
        "the primary retrieval candidate is the persisted snapshot image plus its sidecar",
        "image, snapshot, still-frame, and local DVR segment inputs are the first-version VLM surface for HarborDesk; video understanding uses keyframe sidecars",
        "The Home Device Domain owns discovery, preview, share-link, inspect, and control actions in this shell.",
        "The UI keeps retrieval evidence separate from runtime control, and keeps device control separate from IM and HarborOS system control.",
        "Waiting for AIoT boundary summary from the admin-plane: owned device actions, non-regression notes, retrieval/control separation, and HarborOS non-ownership.",
        "owned_actions=discover / snapshot / share_link / inspect / control",
        "retrieval/control separation stays explicit: HarborOS does not own device-native control.",
        "non_regression=route_key stays opaque routing metadata · resume_token stays business-flow continuation",
        "`inspect` and `control` remain runtime-only and are not claimed by HarborOS executors or retrieval logic",
        "TP-Link/Tapo",
        "TP-Link/Tapo candidates now prefer `/stream1` and `/stream2` before the generic RTSP defaults.",
        "no native snapshot URL was confirmed from the current session context",
        "Home Device Domain",
        "continuous video is first represented as rolling media segments with keyframe sidecars; it does not create a DVR-specific VLM, embedding, reranker, or answer chain.",
        "VLM first now covers still images, snapshots, and DVR keyframe sidecars; audio transcript extraction remains pending.",
    ]

    content = " ".join("\n".join([camera, ingest, retrieval, acceptance, ui_index, ui_app]).split())
    assert all(phrase in content for phrase in required_phrases)
