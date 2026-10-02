import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import qs.Commons
import qs.Ui
import "Model.js" as Model

// One bar icon and one panel: nexthop tabs, omaconnect pairing, hyprmoncfg display.
// Service.qml is the only spawn site — ctl, never HTTPS, never the token.
Panel {
  id: root
  moduleName: "punktfunk"
  manageIpc: true
  ipcTarget: "punktfunk"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family
  readonly property color accent: bar && bar.accent ? bar.accent : Color.accent
  readonly property color hoverFill: bar ? Style.hoverFillFor(bar.foreground, Color.accent) : "transparent"
  readonly property color selectedFill: bar ? Style.selectedFillFor(bar.foreground, Color.accent) : "transparent"

  readonly property bool needsYou: service.pending > 0 || service.pinPending
  readonly property bool live: Model.sessionLive(service.state, service.sessions, service.games)
  readonly property bool showArm: Model.showArmRow(service.state)
  readonly property var sessionActionModel: Model.sessionActions(live, service.games.length > 0)
  readonly property var devices: service.nativeClients.concat(service.gamestreamClients)
  readonly property var presetList: Model.allPresets(service.displayPresets, service.customPresets)
  readonly property var displayModel: Model.displayRows(presetList)
  readonly property var facts: Model.sessionFacts({
    state: service.state,
    sessions: service.sessions,
    games: service.games,
    stream: service.stream,
    armed: service.armed,
    summary: service.summary
  })
  readonly property var spark: Model.sparkPoints(service.history, "target")
  readonly property var incoming: Model.incomingPair(service.pendingDevices)
  readonly property var extraPending: Model.remainingPair(service.pendingDevices)
  readonly property var policy: Model.policyRows(service.displayEffective)
  readonly property var pillars: Model.statsPillars({
    stream: service.stream,
    statsSample: service.statsSample,
    history: service.history
  })
  readonly property var charts: Model.chartSeries(service.captureArmed)

  property int currentTab: 0
  readonly property string tab: Model.tabId(currentTab)
  readonly property var tabNames: Model.TAB_NAMES

  property string focusSection: "header"
  property int selectedIndex: 0
  property bool cursorActive: false
  property bool actionFocused: false
  property int phraseIndex: 0
  property bool pinEditing: false
  property string unpairFingerprint: ""
  property int pinFocusTick: 0
  readonly property bool headerHasCursor: cursorActive && focusSection === "header"
  readonly property string heroPhraseText: Model.heroPhrase(phraseIndex)
  readonly property string toggleHint: service.hostEnabled ? "Stop the host" : "Start the host"

  readonly property color glyphColor: {
    if (service.pinMismatch || needsYou) return urgent
    return service.state === "stopped" ? Qt.darker(barForeground, 1.55) : barForeground
  }

  readonly property var cursorSections: {
    if (tab === "overview") return sessionActionModel.length > 0 ? ["session"] : []
    if (tab === "pair") {
      var rows = []
      if (showArm) rows.push("arm")
      if (service.pinDevices.length > 0) rows.push("pin")
      if (incoming) rows.push("incoming")
      if (extraPending.length > 0) rows.push("pending")
      return rows
    }
    if (tab === "devices") return devices.length > 0 ? ["devices"] : []
    if (tab === "display") return displayModel.length > 0 ? ["display"] : []
    if (tab === "stats") return ["capture"]
    return []
  }

  function sectionCount(section) {
    if (section === "session") return sessionActionModel.length
    if (section === "arm" || section === "incoming" || section === "capture") return 1
    if (section === "pin") return service.pinDevices.length
    if (section === "pending") return extraPending.length
    if (section === "devices") return devices.length
    if (section === "display") return displayModel.length
    return 0
  }

  Service { id: service }

  function syncTab() {
    if (tab === "display") service.refreshDisplays()
    if (tab === "devices" || tab === "pair") service.refreshClients()
    if ((tab === "stats" || tab === "overview") && service.state === "streaming")
      service.refreshStats()
  }

  onOpenedChanged: if (opened) {
    cursorActive = false
    actionFocused = false
    unpairFingerprint = ""
    if (panelFlick) panelFlick.contentY = 0
    if (needsYou) currentTab = Model.tabIndex("pair")
    focusSection = "header"
    selectedIndex = 0
    service.refresh()
    service.refreshClients()
    service.refreshDisplays()
    syncTab()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  onCurrentTabChanged: {
    actionFocused = false
    unpairFingerprint = ""
    if (panelFlick) panelFlick.contentY = 0
    var sections = cursorSections
    if (cursorActive && sections.length > 0) {
      focusSection = sections[0]
      selectedIndex = 0
    } else {
      focusSection = "header"
      selectedIndex = 0
    }
    syncTab()
  }

  function ensureCursor() {
    if (focusSection === "header") return
    var sections = cursorSections
    if (!sections.length) { focusSection = "header"; return }
    if (sections.indexOf(focusSection) < 0) {
      focusSection = sections[0]
      selectedIndex = 0
      actionFocused = false
      return
    }
    var n = sectionCount(focusSection)
    if (n <= 0) { focusSection = "header"; return }
    if (selectedIndex > n - 1) selectedIndex = n - 1
    if (selectedIndex < 0) selectedIndex = 0
  }

  function moveCursor(delta) {
    var sections = cursorSections
    if (focusSection === "header") {
      if (delta > 0 && sections.length > 0) {
        focusSection = sections[0]
        selectedIndex = 0
        actionFocused = false
      }
      return
    }
    if (!sections.length) { focusSection = "header"; return }
    var sIdx = sections.indexOf(focusSection)
    if (sIdx < 0) { focusSection = sections[0]; selectedIndex = 0; return }
    var idx = selectedIndex
    var max = sectionCount(focusSection) - 1
    if (delta > 0) {
      if (idx < max) { selectedIndex = idx + 1; actionFocused = false; return }
      if (sIdx < sections.length - 1) {
        focusSection = sections[sIdx + 1]
        selectedIndex = 0
        actionFocused = false
      }
    } else {
      if (idx > 0) { selectedIndex = idx - 1; actionFocused = false; return }
      if (sIdx > 0) {
        focusSection = sections[sIdx - 1]
        selectedIndex = sectionCount(focusSection) - 1
        actionFocused = false
      } else {
        focusSection = "header"
        actionFocused = false
      }
    }
  }

  function activateCursor() {
    if (focusSection === "header") {
      service.setHostEnabled(!service.hostEnabled)
      settle.restart()
      return
    }
    if (focusSection === "session") {
      var act = sessionActionModel[selectedIndex]
      if (!act) return
      service.run([act.id === "end" ? "end-game" : "stop-session"], function() { service.refresh() })
      return
    }
    if (focusSection === "arm") {
      service.run(service.armed ? ["pair", "disarm"] : ["pair", "arm"], function() { service.refresh() })
      return
    }
    if (focusSection === "pin") {
      pinFocusTick++
      return
    }
    if (focusSection === "incoming" && incoming) {
      if (actionFocused) rejectPair(incoming)
      else acceptPair(incoming)
      return
    }
    if (focusSection === "pending") {
      var extra = extraPending[selectedIndex]
      if (!extra) return
      if (actionFocused) rejectPair(extra)
      else acceptPair(extra)
      return
    }
    if (focusSection === "devices") {
      var dev = devices[selectedIndex]
      if (!dev || !dev.fingerprint) return
      if (unpairFingerprint === dev.fingerprint) {
        service.run(["unpair", dev.fingerprint], function() {
          unpairFingerprint = ""
          service.refreshClients(); service.refresh()
        })
      } else {
        unpairFingerprint = dev.fingerprint
      }
      return
    }
    if (focusSection === "display") {
      var drow = displayModel[selectedIndex]
      if (!drow) return
      if (drow.kind === "preset") service.setDisplayPreset(drow.preset.id)
      else { service.setCaptureMode(drow.kind); settle.restart() }
      return
    }
    if (focusSection === "capture") service.setCapture(!service.captureArmed)
  }

  function rejectPair(device) {
    if (!device) return
    service.run(["deny", String(device.id)], function() { service.refresh() })
  }

  function acceptPair(device) {
    if (!device) return
    service.run(["approve", String(device.id)], function() {
      service.refresh(); service.refreshClients()
    })
  }

  function scrollItemIntoView(item) {
    if (!panelFlick || !item) return
    Qt.callLater(function() {
      if (!item) return
      var margin = Style.space(6)
      var point = item.mapToItem(panelFlick.contentItem, 0, 0)
      var top = point.y
      var bottom = top + item.height
      var viewTop = panelFlick.contentY
      var viewBottom = viewTop + panelFlick.height
      var maxY = Math.max(0, panelFlick.contentHeight - panelFlick.height)
      if (top < viewTop + margin) panelFlick.contentY = Math.max(0, top - margin)
      else if (bottom > viewBottom - margin)
        panelFlick.contentY = Math.min(maxY, bottom + margin - panelFlick.height)
    })
  }

  function setHeaderCursor() {
    cursorActive = true
    focusSection = "header"
    selectedIndex = 0
    actionFocused = false
  }

  function setSectionCursor(section, index) {
    cursorActive = true
    focusSection = section
    selectedIndex = index
    actionFocused = false
  }

  onSelectedIndexChanged: ensureCursor()
  onFocusSectionChanged: ensureCursor()
  onCursorSectionsChanged: ensureCursor()

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      Item {
        LensMark {
          anchors.centerIn: parent
          markSize: Style.space(11)
          markColor: root.glyphColor
          filled: service.state === "streaming"
          visible: !service.pinMismatch
        }
        Text {
          anchors.centerIn: parent
          visible: service.pinMismatch
          text: "󰀦"
          color: root.glyphColor
          font.family: root.fontFamily
          font.pixelSize: Style.space(11)
        }
        Rectangle {
          visible: root.needsYou
          anchors { right: parent.right; top: parent.top }
          width: Style.space(6); height: width; radius: width / 2
          color: root.urgent
        }
      }
    }
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton) {
        if (service.state === "streaming")
          service.run(["stop-session"], function() { service.refresh() })
        else
          service.openConsole()
      } else {
        root.toggle()
      }
    }
  }

  Timer {
    id: statsPoll
    interval: 2000
    repeat: true
    triggeredOnStart: true
    running: root.opened && service.state === "streaming"
             && (root.tab === "stats" || root.tab === "overview")
    onTriggered: service.refreshStats()
  }

  Timer {
    id: settle
    interval: 1500
    onTriggered: {
      service.refresh()
      service.refreshClients()
      service.refreshDisplays()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(524))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      blocked: root.pinEditing
      onMoveRequested: function(dx, dy) {
        if (dx !== 0) {
          root.currentTab = Model.nextTab(root.currentTab, dx)
          return
        }
        if (!root.cursorActive) { root.cursorActive = true; return }
        if (dy !== 0) root.moveCursor(dy)
      }
      onActivateRequested: if (root.cursorActive) root.activateCursor()
      onCloseRequested: {
        if (root.unpairFingerprint) { root.unpairFingerprint = ""; return }
        root.close()
      }
      onTabRequested: function(direction) { root.switchPanel(direction) }

      Keys.onPressed: function(event) {
        if (event.key >= Qt.Key_1 && event.key <= Qt.Key_5) {
          root.currentTab = event.key - Qt.Key_1
          event.accepted = true
        } else if (event.key === Qt.Key_X && (root.focusSection === "incoming" || root.focusSection === "pending")) {
          if (root.focusSection === "incoming") root.rejectPair(root.incoming)
          else root.rejectPair(root.extraPending[root.selectedIndex])
          event.accepted = true
        } else if ((event.key === Qt.Key_Y || event.text === "y") && root.unpairFingerprint) {
          root.activateCursor()
          event.accepted = true
        } else if ((event.key === Qt.Key_C || event.text === "c") && root.unpairFingerprint) {
          root.unpairFingerprint = ""
          event.accepted = true
        }
      }

      Flickable {
        id: panelFlick
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        flickableDirection: Flickable.VerticalFlick
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: column
          width: panelFlick.width
          spacing: Style.space(12)

          Item {
            id: header
            width: parent.width
            implicitHeight: hero.implicitHeight
            readonly property bool ringVisible: root.headerHasCursor
            function focusHero() { root.setHeaderCursor() }

            PanelHero {
              id: hero
              width: parent.width
              title: Model.heroTitle(service.state, service.summary.client_name)
              meta: Model.heroMeta(service.state, root.heroPhraseText)
              detail: service.stream ? String(service.stream.codec || "").toUpperCase() : ""
              foreground: root.foreground
              fontFamily: root.fontFamily
              iconOpacity: service.hostEnabled ? 1.0 : 0.5
              iconComponent: Component {
                LensMark {
                  markSize: Style.font.display
                  markColor: service.state === "stopped" ? root.dim : root.foreground
                  filled: service.state === "streaming"
                }
              }
              trailingControl: Component {
                ToggleSwitch {
                  id: powerSwitch
                  checked: service.hostEnabled
                  busy: settle.running
                  hasCursor: header.ringVisible
                  foreground: hero.foreground
                  onHovered: function(on) { if (on) header.focusHero() }
                  onToggled: {
                    service.setHostEnabled(!service.hostEnabled)
                    settle.restart()
                  }
                  PanelToolTip {
                    visible: powerSwitch.containsMouse
                    text: root.toggleHint
                    fontFamily: hero.fontFamily
                  }
                }
              }
            }
          }

          Text {
            visible: service.pinMismatch
            width: parent.width
            wrapMode: Text.Wrap
            color: root.urgent
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            text: "The process answering the management port did not present this host's certificate, so no credential was sent. Either the host regenerated its identity, or something else is on that port."
          }

          Row {
            width: parent.width
            spacing: Style.space(6)

            Repeater {
              model: root.tabNames

              Rectangle {
                id: tabButton
                required property string modelData
                required property int index
                readonly property bool selected: root.currentTab === index
                readonly property string tabId: Model.tabId(index)

                width: (parent.width - Style.space(6) * (root.tabNames.length - 1)) / root.tabNames.length
                height: Style.space(26)
                color: selected
                  ? Style.selectedFillFor(root.foreground, Color.accent)
                  : (tabHover.hovered
                     ? Style.hoverFillFor(root.foreground, Color.accent)
                     : Style.normalFillFor(root.foreground, Color.accent))
                border.width: selected ? 0 : Style.normalBorderWidth
                border.color: Style.normalBorderFor(root.foreground, Color.accent)

                Text {
                  anchors.centerIn: parent
                  text: Model.tabLabel(tabButton.tabId, service.pending)
                  color: tabButton.selected ? root.foreground : root.dim
                  font.family: root.fontFamily
                  font.pixelSize: Style.font.bodySmall
                }

                HoverHandler { id: tabHover }
                TapHandler { onTapped: root.currentTab = tabButton.index }
              }
            }
          }

          PanelSeparator { foreground: root.foreground }

          OverviewTab { visible: root.tab === "overview"; width: parent.width }
          PairTab { visible: root.tab === "pair"; width: parent.width }
          DevicesTab { visible: root.tab === "devices"; width: parent.width }
          DisplayTab { visible: root.tab === "display"; width: parent.width }
          StatsTab { visible: root.tab === "stats"; width: parent.width }
        }
      }
    }
  }

  Timer {
    id: phraseTimer
    interval: 2800
    running: root.opened && service.state === "streaming"
    repeat: true
    onTriggered: phraseSwap.restart()
  }

  SequentialAnimation {
    id: phraseSwap
    PropertyAnimation {
      target: hero; property: "metaOpacity"
      to: 0.0; duration: 180; easing.type: Easing.OutQuad
    }
    ScriptAction {
      script: root.phraseIndex = (root.phraseIndex + 1) % Model.heroPhraseCount()
    }
    PropertyAnimation {
      target: hero; property: "metaOpacity"
      to: 1.0; duration: 260; easing.type: Easing.InQuad
    }
  }

  component LensMark: Item {
    id: lens
    property color markColor
    property bool filled: false
    property int markSize: Style.space(11)
    implicitWidth: markSize
    implicitHeight: markSize
    width: markSize
    height: markSize

    Canvas {
      id: mark
      anchors.fill: parent
      onPaint: {
        var ctx = getContext("2d"); ctx.reset()
        var u = width / 100
        ctx.strokeStyle = String(lens.markColor)
        ctx.fillStyle = String(lens.markColor)
        ctx.lineWidth = 10 * u
        ctx.beginPath(); ctx.arc(34 * u, 66 * u, 29 * u, 0, 2 * Math.PI); ctx.stroke()
        ctx.beginPath(); ctx.arc(66 * u, 34 * u, 29 * u, 0, 2 * Math.PI); ctx.stroke()
        if (lens.filled) {
          ctx.save()
          ctx.beginPath(); ctx.arc(34 * u, 66 * u, 29 * u, 0, 2 * Math.PI); ctx.clip()
          ctx.beginPath(); ctx.arc(66 * u, 34 * u, 29 * u, 0, 2 * Math.PI); ctx.fill()
          ctx.restore()
        }
      }
      Connections {
        target: lens
        function onMarkColorChanged() { mark.requestPaint() }
        function onFilledChanged() { mark.requestPaint() }
      }
    }
  }

  component ScorePillar: Column {
    id: pillar
    property string label: ""
    property string text: "—"
    property string note: ""
    property real meter: 0
    spacing: Style.space(5)

    Text {
      text: pillar.label
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      font.letterSpacing: 1
    }
    Text {
      text: pillar.text
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.title
      font.weight: Font.Bold
    }
    Rectangle {
      width: parent.width
      height: Math.max(2, Style.space(3))
      color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.12)
      Rectangle {
        height: parent.height
        width: parent.width * Math.max(0, Math.min(1, pillar.meter))
        color: root.accent
        Behavior on width { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }
      }
    }
    Text {
      width: parent.width
      text: pillar.note
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      elide: Text.ElideRight
    }
  }

  component SparkChart: Column {
    id: chart
    property string label: ""
    property string unit: ""
    property int digits: 1
    property var pts: []
    spacing: 2

    readonly property var last: Model.seriesLast(pts)

    RowLayout {
      width: parent.width
      Text {
        text: chart.label
        color: root.dim
        font.family: root.fontFamily
        font.pixelSize: Style.font.caption
      }
      Item { Layout.fillWidth: true }
      Text {
        text: chart.last === null ? "—" : Number(chart.last).toFixed(chart.digits) + " " + chart.unit
        color: root.foreground
        font.family: "monospace"
        font.pixelSize: Style.font.caption
      }
    }

    Canvas {
      id: spark
      width: parent.width
      height: Style.space(36)
      readonly property var pts: chart.pts
      onPtsChanged: requestPaint()
      onPaint: {
        var ctx = getContext("2d"); ctx.reset()
        var vals = pts
        if (!vals || vals.length < 2) return
        var lo = Math.min.apply(null, vals), hi = Math.max.apply(null, vals)
        if (hi - lo < 1e-6) { lo -= 1; hi += 1 }
        var pad = Style.space(3)
        var h = height - pad * 2
        var xOf = function(n) { return n / (vals.length - 1) * width }
        var yOf = function(v) { return pad + h - (v - lo) / (hi - lo) * h }
        ctx.beginPath()
        ctx.moveTo(xOf(0), yOf(vals[0]))
        for (var i = 1; i < vals.length; i++) ctx.lineTo(xOf(i), yOf(vals[i]))
        ctx.lineTo(xOf(vals.length - 1), height)
        ctx.lineTo(xOf(0), height)
        ctx.closePath()
        ctx.fillStyle = Qt.rgba(root.accent.r, root.accent.g, root.accent.b, 0.16)
        ctx.fill()
        ctx.beginPath()
        ctx.moveTo(xOf(0), yOf(vals[0]))
        for (i = 1; i < vals.length; i++) ctx.lineTo(xOf(i), yOf(vals[i]))
        ctx.strokeStyle = String(root.accent)
        ctx.lineWidth = 1.5
        ctx.lineJoin = "round"
        ctx.stroke()
      }
      Connections {
        target: root
        function onAccentChanged() { spark.requestPaint() }
      }
    }
  }

  component OverviewTab: Column {
    spacing: Style.space(10)

    GridLayout {
      visible: root.facts.length > 0
      width: parent.width
      columns: 2
      columnSpacing: Style.spacing.md
      rowSpacing: Style.spacing.sm

      Repeater {
        model: root.facts
        Column {
          Layout.fillWidth: true
          Layout.preferredWidth: 1
          spacing: 0
          Text {
            width: parent.width
            text: modelData.k
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
          Text {
            width: parent.width
            text: modelData.v
            color: root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
          }
        }
      }
    }

    Text {
      visible: root.live && service.games.length === 0
      width: parent.width
      wrapMode: Text.Wrap
      text: "No game was launched through Punktfunk — this is the desktop."
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }

    Repeater {
      model: service.games
      Column {
        width: parent.width
        spacing: 0
        Text {
          width: parent.width
          text: modelData.title || "Desktop"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }
        Text {
          width: parent.width
          text: (modelData.client || "—") + " · " + (modelData.plane || "")
              + (modelData.state === "grace" ? " · reconnecting" : "")
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }
    }

    Column {
      id: sessionColumn
      width: parent.width
      spacing: Style.space(6)

      Repeater {
        model: root.sessionActionModel
        ActionRow {
          required property var modelData
          required property int index
          width: sessionColumn.width
          rowIndex: index
          label: modelData.label
        }
      }
    }

    Row {
      id: overviewPillars
      visible: root.pillars.length > 0
      width: parent.width
      spacing: Style.space(14)
      readonly property real cell: (width - Style.space(14) * 2) / 3

      Repeater {
        model: root.pillars
        ScorePillar {
          required property var modelData
          width: overviewPillars.cell
          label: modelData.label
          text: modelData.text
          note: modelData.note
          meter: modelData.meter
        }
      }
    }

    SparkChart {
      visible: root.live
      width: parent.width
      label: "Target"
      unit: "Mbps"
      pts: root.spark
    }

    Text {
      visible: root.live && root.spark.length < 2
      width: parent.width
      text: "collecting…"
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }
  }

  component PairTab: Column {
    spacing: Style.space(10)

    Rectangle {
      visible: !!root.incoming
      width: parent.width
      implicitHeight: incomingContent.implicitHeight + Style.space(16)
      radius: Style.cornerRadius
      color: Qt.rgba(Color.accent.r, Color.accent.g, Color.accent.b, 0.14)
      border.color: Color.accent
      border.width: 1

      Column {
        id: incomingContent
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.margins: Style.space(8)
        spacing: Style.space(6)

        Text {
          width: parent.width
          text: root.incoming ? "Pairing request from " + (root.incoming.name || "(unnamed device)") : "Pairing request"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          font.bold: true
          elide: Text.ElideRight
        }
        Text {
          width: parent.width
          text: Model.tail(root.incoming ? root.incoming.fingerprint : "")
              + " · " + ((root.incoming && root.incoming.age_secs) || 0) + "s ago"
          color: root.dim
          font.family: "monospace"
          font.pixelSize: Style.font.caption
        }
        Row {
          spacing: Style.space(6)
          Button {
            text: "Accept"
            selected: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            onClicked: root.acceptPair(root.incoming)
          }
          Button {
            text: "Reject"
            foreground: root.foreground
            fontFamily: root.fontFamily
            onClicked: root.rejectPair(root.incoming)
          }
        }
      }
    }

    Rectangle {
      visible: service.armed && service.pairingPin.length > 0
      width: parent.width
      implicitHeight: pinVerify.implicitHeight + Style.space(16)
      radius: Style.cornerRadius
      color: Style.hoverFillFor(root.foreground, Color.accent)
      border.color: root.accent
      border.width: 1

      Column {
        id: pinVerify
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.margins: Style.space(8)
        spacing: Style.space(4)

        Text {
          width: parent.width
          text: "Verify on both devices"
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          font.letterSpacing: 1
        }
        Text {
          width: parent.width
          text: service.pairingPin
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.title
          font.weight: Font.Bold
        }
      }
    }

    CursorSurface {
      visible: root.showArm
      width: parent.width
      hasCursor: root.cursorActive && root.focusSection === "arm"
      foreground: root.foreground
      fill: root.hoverFill
      implicitHeight: armInner.implicitHeight + Style.spacing.xl

      Item {
        id: armInner
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        anchors.leftMargin: Style.space(10)
        anchors.rightMargin: Style.space(8)
        implicitHeight: Math.max(armLabels.implicitHeight, armSwitch.implicitHeight)

        Column {
          id: armLabels
          anchors.left: parent.left
          anchors.right: armSwitch.left
          anchors.rightMargin: Style.space(8)
          anchors.verticalCenter: parent.verticalCenter
          spacing: Style.space(1)

          Text {
            width: parent.width
            text: service.armed ? "Pairing window open" : "Pairing window"
            color: root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
          }
          Text {
            width: parent.width
            wrapMode: Text.Wrap
            text: service.armed
                    ? (service.pairingPin ? "Enter " + service.pairingPin + " on the device" : "Add this host on the device")
                    : "Open a pairing window, then add this host on the device."
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
          }
        }

        ToggleSwitch {
          id: armSwitch
          anchors.right: parent.right
          anchors.verticalCenter: parent.verticalCenter
          checked: service.armed
          hasCursor: root.cursorActive && root.focusSection === "arm"
          foreground: root.foreground
          onHovered: function(on) { if (on) root.setSectionCursor("arm", 0) }
          onToggled: {
            root.setSectionCursor("arm", 0)
            root.activateCursor()
          }
        }

        MouseArea {
          anchors.left: parent.left
          anchors.right: armSwitch.left
          anchors.top: parent.top
          anchors.bottom: parent.bottom
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onEntered: root.setSectionCursor("arm", 0)
          onClicked: {
            root.setSectionCursor("arm", 0)
            root.activateCursor()
          }
        }
      }
    }

    Column {
      id: pinList
      visible: service.pinDevices.length > 0
      width: parent.width
      spacing: Style.space(6)

      Text {
        width: parent.width
        text: "Moonlight is waiting for a PIN"
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
      }

      Repeater {
        model: service.pinDevices
        PinRow {
          required property var modelData
          required property int index
          width: pinList.width
          device: modelData
          rowIndex: index
        }
      }
    }

    Column {
      id: extraList
      visible: root.extraPending.length > 0
      width: parent.width
      spacing: Style.space(6)

      Repeater {
        model: root.extraPending
        PendingRow {
          required property var modelData
          required property int index
          width: extraList.width
          device: modelData
          rowIndex: index
        }
      }
    }

    Text {
      visible: !root.showArm && !root.incoming && service.pinDevices.length === 0
      width: parent.width
      wrapMode: Text.Wrap
      text: "Start the host to open a pairing window."
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }
  }

  component DevicesTab: Column {
    spacing: Style.space(10)

    PanelSectionHeader {
      visible: root.devices.length > 0
      text: "DEVICES · " + root.devices.length
      foreground: root.foreground
      fontFamily: root.fontFamily
    }

    Text {
      visible: root.devices.length === 0
      width: parent.width
      wrapMode: Text.Wrap
      text: "No devices are paired yet. Open a pairing window, then add this host on the device."
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }

    Column {
      id: deviceColumn
      width: parent.width
      spacing: Style.space(6)

      Repeater {
        model: root.devices
        DeviceRow {
          required property var modelData
          required property int index
          width: deviceColumn.width
          device: modelData
          rowIndex: index
        }
      }
    }
  }

  component DisplayTab: Column {
    spacing: Style.space(10)

    Item {
      width: parent.width
      implicitHeight: Math.max(displayHeroIcon.implicitHeight, displayHeroLabels.implicitHeight)

      Text {
        id: displayHeroIcon
        anchors.left: parent.left
        anchors.verticalCenter: parent.verticalCenter
        text: service.captureMode === "mirror" ? "󰍹" : "󰍺"
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.display
      }

      Column {
        id: displayHeroLabels
        anchors.left: displayHeroIcon.right
        anchors.leftMargin: Style.space(14)
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(1)

        Text {
          width: parent.width
          text: service.captureMode === "mirror" ? "This screen" : "Dedicated display"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.title
          font.bold: true
          elide: Text.ElideRight
        }
        Text {
          width: parent.width
          text: service.displayPreset || "no preset"
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          font.bold: true
          elide: Text.ElideRight
        }
      }
    }

    Rectangle {
      width: parent.width
      height: Style.space(150)
      radius: Style.cornerRadius
      color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.025)
      border.width: 1
      border.color: Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.18)

      Row {
        id: canvasRow
        anchors.fill: parent
        anchors.margins: Style.space(10)
        spacing: Style.space(10)

        Repeater {
          model: [
            { kind: "dedicated", label: "Dedicated", detail: "Virtual display at the client's size" },
            { kind: "mirror", label: "This screen", detail: "The primary monitor on this box" }
          ]

          Rectangle {
            required property var modelData
            required property int index
            readonly property bool current: service.captureMode === modelData.kind
            readonly property bool selected: root.cursorActive && root.focusSection === "display" && root.selectedIndex === index
            width: (canvasRow.width - Style.space(10)) / 2
            height: canvasRow.height
            radius: Math.min(Style.cornerRadius, Style.space(5))
            color: current || selected
              ? Style.selectedFillFor(root.foreground, root.accent)
              : Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.045)
            border.width: current ? Math.max(1, Style.normalBorderWidth) : 1
            border.color: current ? root.accent : Qt.rgba(root.foreground.r, root.foreground.g, root.foreground.b, 0.55)

            Column {
              anchors.centerIn: parent
              width: Math.max(0, parent.width - Style.space(10))
              spacing: Style.space(2)

              Text {
                width: parent.width
                text: modelData.label
                color: root.foreground
                font.family: root.fontFamily
                font.pixelSize: Style.font.body
                font.bold: true
                horizontalAlignment: Text.AlignHCenter
                elide: Text.ElideRight
              }
              Text {
                width: parent.width
                text: modelData.detail
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.Wrap
              }
              Text {
                visible: current && !!service.stream
                width: parent.width
                text: service.stream ? service.stream.width + " × " + service.stream.height : ""
                color: root.dim
                font.family: root.fontFamily
                font.pixelSize: Style.font.caption
                horizontalAlignment: Text.AlignHCenter
              }
            }

            MouseArea {
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onEntered: root.setSectionCursor("display", index)
              onClicked: {
                root.setSectionCursor("display", index)
                root.activateCursor()
              }
            }
          }
        }
      }
    }

    GridLayout {
      visible: root.policy.length > 0
      width: parent.width
      columns: 2
      columnSpacing: Style.spacing.md
      rowSpacing: Style.spacing.sm

      Repeater {
        model: root.policy
        Column {
          Layout.fillWidth: true
          Layout.preferredWidth: 1
          spacing: 0
          Text {
            width: parent.width
            text: modelData.k
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.caption
            font.letterSpacing: 1
          }
          Text {
            width: parent.width
            text: modelData.v
            color: root.foreground
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            elide: Text.ElideRight
          }
        }
      }
    }

    PanelSectionHeader {
      visible: root.presetList.length > 0
      text: "PRESETS"
      foreground: root.foreground
      fontFamily: root.fontFamily
    }

    Column {
      id: displayColumn
      width: parent.width
      spacing: Style.space(6)

      Repeater {
        model: root.presetList
        DisplayRow {
          required property var modelData
          required property int index
          width: displayColumn.width
          row: ({ kind: "preset", preset: modelData })
          rowIndex: index + 2
        }
      }
    }

    Text {
      visible: root.presetList.length > 0
      width: parent.width
      wrapMode: Text.Wrap
      text: "Hyprland can keep a named display across a disconnect and recast it for a matching reconnect. Hosts without named-head keep-alive support still tear it down with the session."
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }
  }

  component StatsTab: Column {
    spacing: Style.space(10)

    Text {
      visible: !service.stream
      width: parent.width
      wrapMode: Text.Wrap
      text: service.state === "stopped"
              ? "The host is not running."
              : "Nothing is streaming, so there is nothing to measure."
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }

    Text {
      visible: !!service.stream
      width: parent.width
      text: service.stream
              ? service.stream.width + "×" + service.stream.height + " @ " + service.stream.fps
                + " · " + String(service.stream.codec || "").toUpperCase()
              : ""
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
    }

    Row {
      id: statsPillars
      visible: root.pillars.length > 0
      width: parent.width
      spacing: Style.space(14)
      readonly property real cell: (width - Style.space(14) * 2) / 3

      Repeater {
        model: root.pillars
        ScorePillar {
          required property var modelData
          width: statsPillars.cell
          label: modelData.label
          text: modelData.text
          note: modelData.note
          meter: modelData.meter
        }
      }
    }

    Text {
      visible: !!service.stream && Number(service.stream.time_to_first_frame_ms || 0) > 0
      width: parent.width
      text: service.stream.time_to_first_frame_ms + " ms to the first frame"
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }

    Text {
      visible: !!service.statsMeta && !!service.statsMeta.encoder_backend
      width: parent.width
      text: (service.statsMeta.encoder_backend || "")
            + (service.statsMeta.gpu ? " · " + service.statsMeta.gpu : "")
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
      elide: Text.ElideRight
    }

    PanelSeparator { foreground: root.foreground }

    RowLayout {
      width: parent.width
      PanelSectionHeader {
        Layout.fillWidth: true
        text: service.captureArmed
                ? "FRAME TIMINGS · " + service.captureSamples + " samples"
                : "FRAME TIMINGS"
        foreground: root.foreground
        fontFamily: root.fontFamily
      }
      PanelActionButton {
        iconText: service.captureArmed ? "󰓛" : "󰑊"
        tooltipText: service.captureArmed
                       ? "Stop recording and save the capture"
                       : "Record frame timings"
        foreground: service.captureArmed ? root.urgent : root.foreground
        hasCursor: root.cursorActive && root.focusSection === "capture"
        onClicked: service.setCapture(!service.captureArmed)
      }
    }

    Text {
      visible: !service.captureArmed
      width: parent.width
      wrapMode: Text.Wrap
      text: "Per-frame drops and stage timings are only sampled while a capture is recording. Stopping saves it; the console graphs the saved ones."
      color: root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.caption
    }

    Repeater {
      model: root.charts
      SparkChart {
        required property var modelData
        width: parent.width
        label: modelData.label
        unit: modelData.unit
        digits: modelData.digits
        pts: Model.sparkPoints(service.history, modelData.key)
      }
    }
  }

  component ActionRow: CursorSurface {
    id: actionRow
    property int rowIndex: 0
    property string label: ""

    hasCursor: root.cursorActive && root.focusSection === "session" && root.selectedIndex === rowIndex
    onHasCursorChanged: if (hasCursor) root.scrollItemIntoView(actionRow)
    foreground: root.foreground
    fill: root.hoverFill
    implicitHeight: actionInner.implicitHeight + Style.spacing.xl

    Text {
      id: actionInner
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      text: actionRow.label
      color: root.foreground
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
      elide: Text.ElideRight
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: root.setSectionCursor("session", actionRow.rowIndex)
      onClicked: {
        root.setSectionCursor("session", actionRow.rowIndex)
        root.activateCursor()
      }
    }
  }

  component PinRow: CursorSurface {
    id: pinRow
    property var device: null
    property int rowIndex: 0
    readonly property bool rowSelected: root.cursorActive && root.focusSection === "pin" && root.selectedIndex === rowIndex

    function focusPin() { pinField.forceActiveFocus() }

    function submitPin() {
      var args = Model.pinArgs(pinField.text, device)
      if (!args.length) return
      service.run(args, function() { pinField.text = ""; service.refresh() })
    }

    hasCursor: rowSelected
    onHasCursorChanged: if (hasCursor) root.scrollItemIntoView(pinRow)
    Connections {
      target: root
      function onPinFocusTickChanged() {
        if (pinRow.rowSelected) pinRow.focusPin()
      }
    }
    foreground: root.foreground
    fill: root.hoverFill
    implicitHeight: pinInner.implicitHeight + Style.spacing.xl

    Item {
      id: pinInner
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(8)
      implicitHeight: Math.max(pinField.implicitHeight, pinBtn.implicitHeight)

      TextField {
        id: pinField
        anchors.left: parent.left
        anchors.right: pinBtn.left
        anchors.rightMargin: Style.space(8)
        anchors.verticalCenter: parent.verticalCenter
        placeholderText: pinRow.device && pinRow.device.uniqueid
                         ? "PIN · " + pinRow.device.uniqueid
                         : "Moonlight PIN"
        onActiveFocusChanged: {
          root.pinEditing = activeFocus
          if (activeFocus) root.setSectionCursor("pin", pinRow.rowIndex)
        }
        Keys.onPressed: function(event) {
          if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            pinRow.submitPin()
            event.accepted = true
          } else if (event.key === Qt.Key_Escape) {
            keyCatcher.forceActiveFocus()
            event.accepted = true
          }
        }
      }

      Button {
        id: pinBtn
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        text: "Submit"
        selected: true
        foreground: root.foreground
        fontFamily: root.fontFamily
        fontSize: Style.font.bodySmall
        onClicked: pinRow.submitPin()
      }
    }
  }

  component PendingRow: CursorSurface {
    id: pendingRow
    property var device: null
    property int rowIndex: 0
    readonly property bool rowSelected: root.cursorActive && root.focusSection === "pending" && root.selectedIndex === rowIndex

    hasCursor: rowSelected
    onHasCursorChanged: if (hasCursor) root.scrollItemIntoView(pendingRow)
    foreground: root.foreground
    fill: root.hoverFill
    implicitHeight: pendingInner.implicitHeight + Style.spacing.xl

    Item {
      id: pendingInner
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(8)
      implicitHeight: Math.max(pendingInfo.implicitHeight, pendingBtns.implicitHeight)

      Column {
        id: pendingInfo
        anchors.left: parent.left
        anchors.right: pendingBtns.left
        anchors.rightMargin: Style.space(8)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(1)

        Text {
          width: parent.width
          text: (pendingRow.device && pendingRow.device.name) || "(unnamed device)"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }
        Text {
          width: parent.width
          text: Model.tail(pendingRow.device ? pendingRow.device.fingerprint : "")
              + " · " + ((pendingRow.device && pendingRow.device.age_secs) || 0) + "s ago"
          color: root.dim
          font.family: "monospace"
          font.pixelSize: Style.font.caption
        }
      }

      Row {
        id: pendingBtns
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(4)
        Button {
          text: "Accept"
          selected: true
          foreground: root.foreground
          fontFamily: root.fontFamily
          fontSize: Style.font.bodySmall
          onClicked: root.acceptPair(pendingRow.device)
        }
        Button {
          text: "Reject"
          foreground: root.foreground
          fontFamily: root.fontFamily
          fontSize: Style.font.bodySmall
          onClicked: root.rejectPair(pendingRow.device)
        }
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      acceptedButtons: Qt.NoButton
      onEntered: root.setSectionCursor("pending", pendingRow.rowIndex)
    }
  }

  component DeviceRow: CursorSurface {
    id: deviceRow
    property var device: null
    property int rowIndex: 0
    readonly property bool isNative: device && (device.access_level !== undefined || device.name !== undefined)
    readonly property bool rowSelected: root.cursorActive && root.focusSection === "devices" && root.selectedIndex === rowIndex
    readonly property bool confirming: !!(device && device.fingerprint && root.unpairFingerprint === device.fingerprint)

    hasCursor: rowSelected
    onHasCursorChanged: if (hasCursor) root.scrollItemIntoView(deviceRow)
    foreground: root.foreground
    fill: root.hoverFill
    implicitHeight: deviceInner.implicitHeight + Style.spacing.xl

    Item {
      id: deviceInner
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(8)
      implicitHeight: Math.max(deviceInfo.implicitHeight, deviceBtns.implicitHeight)

      Column {
        id: deviceInfo
        anchors.left: parent.left
        anchors.right: deviceBtns.left
        anchors.rightMargin: Style.space(8)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(1)

        Text {
          width: parent.width
          text: (deviceRow.device && (deviceRow.device.name || deviceRow.device.label)) || "(unnamed device)"
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          font.bold: deviceRow.rowSelected
          elide: Text.ElideRight
        }
        Text {
          width: parent.width
          text: Model.tail(deviceRow.device ? deviceRow.device.fingerprint : "")
              + " · " + (deviceRow.isNative ? "punktfunk" : "moonlight")
              + (deviceRow.device && deviceRow.device.access_level ? " · " + deviceRow.device.access_level : "")
          color: root.dim
          font.family: "monospace"
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }

      Row {
        id: deviceBtns
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(4)

        Row {
          visible: deviceRow.confirming
          spacing: Style.space(4)
          Button {
            text: "Confirm"
            selected: true
            foreground: root.foreground
            fontFamily: root.fontFamily
            fontSize: Style.font.bodySmall
            onClicked: {
              if (!deviceRow.device || !deviceRow.device.fingerprint) return
              service.run(["unpair", deviceRow.device.fingerprint], function() {
                root.unpairFingerprint = ""
                service.refreshClients(); service.refresh()
              })
            }
          }
          Button {
            text: "Cancel"
            foreground: root.foreground
            fontFamily: root.fontFamily
            fontSize: Style.font.bodySmall
            onClicked: root.unpairFingerprint = ""
          }
        }

        Button {
          visible: !deviceRow.confirming
          text: "Unpair"
          foreground: root.foreground
          fontFamily: root.fontFamily
          fontSize: Style.font.bodySmall
          onClicked: {
            if (!deviceRow.device || !deviceRow.device.fingerprint) return
            root.setSectionCursor("devices", deviceRow.rowIndex)
            root.unpairFingerprint = deviceRow.device.fingerprint
          }
        }
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      acceptedButtons: Qt.NoButton
      onEntered: root.setSectionCursor("devices", deviceRow.rowIndex)
    }
  }

  component DisplayRow: CursorSurface {
    id: displayRow
    property var row: null
    property int rowIndex: 0
    readonly property var preset: row && row.preset ? row.preset : null
    readonly property bool isCurrent: !!(preset && service.displayPreset === preset.id)
    readonly property string title: (preset && (preset.name || preset.id)) || ""
    readonly property string subtitle: (preset && (preset.summary || (preset.fields ? "Saved preset." : ""))) || ""

    hasCursor: root.cursorActive && root.focusSection === "display" && root.selectedIndex === rowIndex
    onHasCursorChanged: if (hasCursor) root.scrollItemIntoView(displayRow)
    current: isCurrent
    foreground: root.foreground
    fill: root.hoverFill
    currentFill: root.selectedFill
    implicitHeight: displayInner.implicitHeight + Style.spacing.xl

    Row {
      id: displayInner
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(6)
      anchors.rightMargin: Style.space(6)
      spacing: Style.space(8)

      Text {
        text: displayRow.isCurrent ? "󰄬" : " "
        color: root.foreground
        font.family: root.fontFamily
        font.pixelSize: Style.font.body
        width: Style.space(22)
        horizontalAlignment: Text.AlignHCenter
        anchors.verticalCenter: parent.verticalCenter
      }

      Column {
        width: parent.width - Style.space(30)
        anchors.verticalCenter: parent.verticalCenter
        spacing: Style.space(1)

        Text {
          width: parent.width
          text: displayRow.title
          color: displayRow.isCurrent || displayRow.hasCursor ? root.foreground : root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          font.bold: displayRow.isCurrent
          elide: Text.ElideRight
        }
        Text {
          visible: displayRow.subtitle.length > 0
          width: parent.width
          text: displayRow.subtitle
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          wrapMode: Text.Wrap
        }
      }
    }

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: root.setSectionCursor("display", displayRow.rowIndex)
      onClicked: {
        root.setSectionCursor("display", displayRow.rowIndex)
        root.activateCursor()
      }
    }
  }
}
