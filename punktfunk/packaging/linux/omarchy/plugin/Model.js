// Phrases, figures, and which sections the panel shows. QML imports this;
// node:test loads the same file through `module.exports`.

var ACTIVE_PHRASES = [
  "Pushing pixels",
  "Slicing frames",
  "Holding the stream",
  "Guarding the desk",
  "Lighting the couch",
  "Braiding packets",
  "Watching the bitrate",
  "Keeping the latency"
]

function mbps(kbps) {
  return (Number(kbps || 0) / 1000).toFixed(1)
}

function dur(us) {
  var n = Number(us || 0)
  return n >= 1000 ? (n / 1000).toFixed(1) + " ms" : Math.round(n) + " µs"
}

function tail(fp) {
  var s = String(fp || "")
  return s.length > 10 ? "…" + s.slice(-10) : (s || "—")
}

function heroPhraseCount(phrases) {
  return (phrases || ACTIVE_PHRASES).length
}

function heroPhrase(index, phrases) {
  var list = phrases || ACTIVE_PHRASES
  if (!list.length) return ""
  var n = list.length
  var i = ((Number(index) || 0) % n + n) % n
  return list[i]
}

function heroTitle(state, clientName) {
  if (state === "streaming" && clientName) return String(clientName)
  return "Punktfunk"
}

function heroMeta(state, phrase) {
  if (state === "streaming") return String(phrase || "")
  if (state === "idle") return "Ready to stream"
  return "Host is stopped"
}

function captureModeStatus(output) {
  var mode = String(output || "").trim()
  return mode === "dedicated" || mode === "mirror" ? mode : null
}

function sessionLive(state, sessions, games) {
  return state === "streaming" || Number(sessions || 0) > 0 || (games && games.length > 0)
}

function showArmRow(state) {
  return state === "idle" || state === "streaming"
}

function pairingExpanded(needsYou) {
  return !!needsYou
}

function pairingWindow(data) {
  var d = data || {}
  var devices = d.pending || []
  return {
    armed: !!d.armed,
    pairingPin: d.pin || "",
    pinDevices: devices,
    pinPending: !!d.pin_pending && devices.length > 0
  }
}

function sectionVisible(section, snap) {
  var s = snap || {}
  if (section === "session") return true
  if (section === "pairing") return showArmRow(s.state) || !!s.needsYou
  if (section === "devices") return true
  if (section === "display") return true
  return false
}

function visibleSections(snap) {
  var names = ["session", "pairing", "devices", "display"]
  var out = []
  for (var i = 0; i < names.length; i++) {
    if (sectionVisible(names[i], snap)) out.push(names[i])
  }
  return out
}

function sessionFacts(snap) {
  var s = snap || {}
  var stream = s.stream || null
  var summary = s.summary || {}
  if (s.state === "stopped") return []
  if (sessionLive(s.state, s.sessions, s.games) && stream) {
    var out = [
      { k: "Resolution", v: stream.width + " × " + stream.height },
      { k: "Frame rate", v: stream.fps + " fps" },
      { k: "Bitrate", v: mbps(stream.bitrate_kbps) + " Mbps" }
    ]
    if (Number(stream.time_to_first_frame_ms || 0) > 0)
      out.push({ k: "First frame", v: stream.time_to_first_frame_ms + " ms" })
    return out
  }
  return [
    { k: "Devices paired", v: String(summary.native_paired_clients || 0) },
    { k: "Pairing", v: s.armed ? "open" : "closed" },
    { k: "Host", v: summary.version || "—" }
  ]
}

function sessionActions(live, hasGame) {
  if (!live) return []
  var out = [{ id: "stop", label: "Stop the session" }]
  if (hasGame) out.push({ id: "end", label: "End the game" })
  return out
}

function pairingRows(arm, pinDevices, pendingDevices) {
  var out = []
  if (arm) out.push({ kind: "arm" })
  var pins = pinDevices || []
  for (var i = 0; i < pins.length; i++)
    out.push({ kind: "pin", device: pins[i] })
  var list = pendingDevices || []
  for (var j = 0; j < list.length; j++)
    out.push({ kind: "pending", device: list[j] })
  return out
}

function pinArgs(pin, device) {
  var d = device || {}
  var values = [
    String(pin || "").trim(),
    String(d.uniqueid || "").trim(),
    String(d.fingerprint || "").trim(),
    String(d.peer_ip || "").trim()
  ]
  for (var i = 0; i < values.length; i++) {
    if (!values[i]) return []
  }
  return ["pin"].concat(values)
}

function displayRows(presets) {
  var out = [
    { kind: "dedicated", label: "Dedicated display", detail: "Stream a virtual display at the client's size" },
    { kind: "mirror", label: "This screen", detail: "Stream the primary monitor on this box" }
  ]
  var list = presets || []
  for (var i = 0; i < list.length; i++)
    out.push({ kind: "preset", preset: list[i] })
  return out
}

function allPresets(builtIn, custom) {
  return (builtIn || []).concat(custom || [])
}

function sparkPoints(history, key) {
  var rows = history || []
  var out = []
  for (var i = 0; i < rows.length; i++) {
    var v = rows[i] ? rows[i][key] : null
    if (v !== null && v !== undefined) out.push(Number(v))
  }
  return out
}

var TAB_NAMES = ["Overview", "Pair", "Devices", "Display", "Stats"]
var TAB_IDS = ["overview", "pair", "devices", "display", "stats"]

function tabId(index) {
  var i = Number(index) || 0
  if (i < 0 || i >= TAB_IDS.length) return TAB_IDS[0]
  return TAB_IDS[i]
}

function tabIndex(id) {
  var i = TAB_IDS.indexOf(String(id || ""))
  return i < 0 ? 0 : i
}

function nextTab(index, delta) {
  var n = TAB_IDS.length
  return ((Number(index) || 0) + (Number(delta) || 0) % n + n) % n
}

function tabLabel(id, pending) {
  if (id === "pair" && Number(pending || 0) > 0) return "Pair · " + Number(pending)
  var i = tabIndex(id)
  return TAB_NAMES[i]
}

function incomingPair(pendingDevices) {
  var list = pendingDevices || []
  return list.length > 0 ? list[0] : null
}

function remainingPair(pendingDevices) {
  var list = pendingDevices || []
  return list.length > 1 ? list.slice(1) : []
}

function policyRows(effective) {
  var e = effective || {}
  if (!e.topology) return []
  return [
    { k: "Topology", v: String(e.topology) },
    { k: "Identity", v: String(e.identity || "—") },
    { k: "Mode clash", v: String(e.mode_conflict || "—") },
    { k: "Max displays", v: String(e.max_displays || "—") }
  ]
}

function chartSeries(captureArmed) {
  var out = [{ key: "target", label: "Target", unit: "Mbps", digits: 1 }]
  if (captureArmed) {
    out = out.concat([
      { key: "sent", label: "Sent", unit: "Mbps", digits: 1 },
      { key: "fps", label: "New frames", unit: "fps", digits: 1 },
      { key: "encode", label: "Encode p99", unit: "ms", digits: 1 }
    ])
  }
  return out
}

function seriesLast(pts) {
  var list = pts || []
  for (var i = list.length - 1; i >= 0; i--) {
    if (list[i] !== null && list[i] !== undefined) return Number(list[i])
  }
  return null
}

function pillarMeter(history, key, current) {
  if (current === null || current === undefined) return 0
  var pts = sparkPoints(history, key)
  var hi = Number(current)
  for (var i = 0; i < pts.length; i++) hi = Math.max(hi, pts[i])
  if (hi <= 0) return 0
  return Math.max(0, Math.min(1, Number(current) / hi))
}

function encodeMs(statsSample) {
  var stages = statsSample && statsSample.stages ? statsSample.stages : []
  for (var i = 0; i < stages.length; i++) {
    if (stages[i] && stages[i].name === "encode") return Number(stages[i].p99_us || 0) / 1000
  }
  return null
}

function statsPillars(snap) {
  var s = snap || {}
  var stream = s.stream || null
  if (!stream) return []
  var target = Number(stream.bitrate_kbps || 0) / 1000
  var fps = s.statsSample && s.statsSample.fps != null
    ? Number(s.statsSample.fps)
    : Number(stream.fps || 0)
  var encode = encodeMs(s.statsSample)
  var history = s.history || []
  return [
    {
      label: "TARGET",
      text: target.toFixed(1),
      note: "Mbps encoder target",
      meter: pillarMeter(history, "target", target)
    },
    {
      label: "FRAMES",
      text: fps ? String(Math.round(fps)) : "—",
      note: stream.fps ? stream.fps + " fps mode" : "no sample yet",
      meter: pillarMeter(history, "fps", fps || null)
    },
    {
      label: "ENCODE",
      text: encode === null ? "—" : encode.toFixed(1),
      note: encode === null ? "record timings to sample" : "p99 ms",
      meter: pillarMeter(history, "encode", encode)
    }
  ]
}

if (typeof module !== "undefined") {
  module.exports = {
    ACTIVE_PHRASES: ACTIVE_PHRASES,
    TAB_NAMES: TAB_NAMES,
    TAB_IDS: TAB_IDS,
    mbps: mbps,
    dur: dur,
    tail: tail,
    heroPhraseCount: heroPhraseCount,
    heroPhrase: heroPhrase,
    heroTitle: heroTitle,
    heroMeta: heroMeta,
    captureModeStatus: captureModeStatus,
    sessionLive: sessionLive,
    showArmRow: showArmRow,
    pairingExpanded: pairingExpanded,
    pairingWindow: pairingWindow,
    sectionVisible: sectionVisible,
    visibleSections: visibleSections,
    sessionFacts: sessionFacts,
    sessionActions: sessionActions,
    pairingRows: pairingRows,
    pinArgs: pinArgs,
    displayRows: displayRows,
    allPresets: allPresets,
    sparkPoints: sparkPoints,
    tabId: tabId,
    tabIndex: tabIndex,
    nextTab: nextTab,
    tabLabel: tabLabel,
    incomingPair: incomingPair,
    remainingPair: remainingPair,
    policyRows: policyRows,
    chartSeries: chartSeries,
    seriesLast: seriesLast,
    pillarMeter: pillarMeter,
    encodeMs: encodeMs,
    statsPillars: statsPillars
  }
}
