import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui
import "Model.js" as Model

// OmaCal's bar widget: a calendar glyph in the bar, and a popup listing what
// is happening now and what is coming up, in the same visual grammar as the
// stock network/bluetooth panels. All data comes from the feed OmaCal itself
// writes (`~/.local/state/omacal/upcoming.json`, see `src-tauri/src/upcoming.rs`
// in the OmaCal repo) — this widget never touches the app's database or the
// network, so it degrades to a quiet empty state when OmaCal is not running.
Panel {
  id: root
  moduleName: "omacal.upcoming"
  ipcTarget: "omacal.upcoming"
  // Own handler rather than the base's (same pattern as omarchy.dropbox):
  // the extra methods make every popup action scriptable —
  // `omarchy-shell omacal.upcoming syncNow` from a keybinding, a script,
  // or a test.
  manageIpc: false

  IpcHandler {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function openApp(): void { root.openApp() }
    function syncNow(): string { root.syncNow(); return "ok" }
    function quitApp(): string { root.quitApp(); return "ok" }
  }

  readonly property color foreground: bar ? bar.foreground : Color.foreground
  readonly property color urgent: bar ? bar.urgent : Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // The machine clock the popup buckets and counts against. Ticks while the
  // popup is open so "ends in 12 min" and the NOW section stay honest; a
  // slower tick keeps the closed bar icon's state fresh without cost.
  property double nowMs: Date.now()

  readonly property string feedPath: {
    var state = Quickshell.env("XDG_STATE_HOME")
    var home = Quickshell.env("HOME")
    return (state && String(state).length > 0 ? state : home + "/.local/state")
      + "/omacal/upcoming.json"
  }

  property var feed: null
  readonly property var events: feed ? feed.events : null
  readonly property var taskRows: Model.taskRows(feed, nowMs)
  readonly property var panelSections: Model.sections(events, nowMs, root.setting("maxEvents", 12))
  readonly property var runningEvent: Model.current(events, nowMs)
  readonly property var nextEvent: Model.nextAhead(events, nowMs)
  readonly property bool empty: panelSections.length === 0

  // The bar glyph turns urgent-coloured when a meeting is less than ten
  // minutes out — the glanceable version of "wrap this conversation up".
  readonly property bool imminent: nextEvent !== null
    && nextEvent.start_ms - nowMs < 10 * 60000

  property bool cursorActive: false
  property int rowCursor: 0

  // Sections flattened to one keyboard-navigable list of rows.
  readonly property var flatRows: {
    var rows = []
    for (var s = 0; s < panelSections.length; s++)
      for (var r = 0; r < panelSections[s].rows.length; r++)
        rows.push(panelSections[s].rows[r])
    return rows
  }

  function heroMeta() {
    if (runningEvent)
      return Model.title(runningEvent) + " · " + Model.endsText(runningEvent, nowMs)
    if (nextEvent)
      return Model.title(nextEvent) + " · " + Model.leadText(nextEvent.start_ms, nowMs)
    if (!feed) return "Waiting for OmaCal"
    return "Nothing scheduled"
  }

  function openApp() {
    Quickshell.execDetached(["omacal"])
    root.close()
  }

  // The tray menu's other two actions, carried over the app's
  // single-instance channel (OmaCal ≥ 0.1.10) — this widget plus these two
  // is what lets the tray icon be turned off without losing anything.
  function syncNow() {
    Quickshell.execDetached(["omacal", "--sync-now"])
  }

  function quitApp() {
    Quickshell.execDetached(["omacal", "--quit"])
    root.close()
  }

  // A row's primary action: join the call when there is one, otherwise bring
  // up the app — the two things a calendar row in a status bar is for.
  function activateRow(ev) {
    if (!ev) return
    if (ev.conference) {
      Qt.openUrlExternally(ev.conference)
      root.close()
    } else {
      openApp()
    }
  }

  function moveCursor(dy) {
    cursorActive = true
    if (flatRows.length === 0) return
    rowCursor = Math.max(0, Math.min(flatRows.length - 1, rowCursor + dy))
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onOpenedChanged: if (opened) {
    cursorActive = false
    rowCursor = 0
    nowMs = Date.now()
    feedFile.reload()
    Qt.callLater(function() { keyCatcher.forceActiveFocus() })
  }

  FileView {
    id: feedFile
    path: root.feedPath
    watchChanges: true
    printErrors: false
    onLoaded: root.feed = Model.parseFeed(text())
    onFileChanged: reload()
    onLoadFailed: root.feed = null
  }

  Timer {
    interval: root.opened ? 15000 : 60000
    running: true
    repeat: true
    onTriggered: root.nowMs = Date.now()
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    active: root.imminent
    dimmed: root.events === null || root.events.length === 0
    tooltipText: root.heroMeta()
    // The app's own mark, not a generic glyph: with the tray icon off this
    // is omacal's one presence in the bar. Monochrome like its neighbours;
    // urgent-tinted when a meeting is imminent, as the glyph was.
    iconComponent: Component {
      Item {
        OmacalMark {
          anchors.centerIn: parent
          iconSize: Style.space(12)
          color: root.imminent ? root.urgent : button.foreground
        }
      }
    }
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.MiddleButton) root.openApp()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: keyCatcher
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(column.implicitHeight, Style.space(560))

    PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      onMoveRequested: function(dx, dy) {
        if (!root.cursorActive) { root.cursorActive = true; return }
        root.moveCursor(dy)
      }
      onActivateRequested: if (root.cursorActive) root.activateRow(root.flatRows[root.rowCursor])
      onCloseRequested: root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(t) {
        if (t === "o" || t === "O") root.openApp()
        else if (t === "r" || t === "R") feedFile.reload()
        else if (t === "s" || t === "S") root.syncNow()
        else if (t === "q" || t === "Q") root.quitApp()
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

          PanelHero {
            width: parent.width
            title: "OmaCal"
            meta: root.heroMeta()
            foreground: root.foreground
            fontFamily: root.fontFamily
            iconComponent: Component {
              OmacalMark {
                iconSize: Style.font.display
                color: root.foreground
                // The hero can afford the brand's own orange; the bar cannot.
                dotColor: "#F97316"
              }
            }
            trailingControl: Component {
              PanelActionButton {
                iconText: ""
                foreground: root.foreground
                fontFamily: root.fontFamily
                onClicked: root.openApp()

                PanelToolTip {
                  visible: parent.containsMouse
                  text: "Open OmaCal"
                  fontFamily: root.fontFamily
                }
              }
            }
          }

          // Feed missing entirely: OmaCal has never run (or never on this
          // version). Say what to do, not just that there is nothing.
          Text {
            visible: root.feed === null
            width: parent.width
            text: "No calendar data yet.\nStart OmaCal to populate this panel."
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            horizontalAlignment: Text.AlignHCenter
            wrapMode: Text.WordWrap
          }

          Text {
            visible: root.feed !== null && root.empty && root.taskRows.length === 0
            width: parent.width
            text: "Nothing scheduled in the next two weeks."
            color: root.dim
            font.family: root.fontFamily
            font.pixelSize: Style.font.body
            horizontalAlignment: Text.AlignHCenter
          }

          Repeater {
            model: root.panelSections

            Column {
              id: sectionColumn
              required property var modelData
              required property int index
              width: column.width
              spacing: Style.space(6)

              // Index of this section's first row in the flattened cursor
              // list, so each row can tell whether it holds the cursor.
              readonly property int rowBase: {
                var base = 0
                for (var s = 0; s < index; s++)
                  base += root.panelSections[s].rows.length
                return base
              }

              PanelSeparator {
                visible: sectionColumn.index > 0
                foreground: root.foreground
              }

              PanelSectionHeader {
                text: sectionColumn.modelData.title
                foreground: sectionColumn.modelData.title === "NOW" ? root.urgent : root.foreground
                fontFamily: root.fontFamily
              }

              Repeater {
                model: sectionColumn.modelData.rows

                EventRow {
                  required property var modelData
                  required property int index
                  width: sectionColumn.width
                  event: modelData
                  flatIndex: sectionColumn.rowBase + index
                  sectionTitle: sectionColumn.modelData.title
                }
              }
            }
          }

          // Overdue-or-imminent tasks (OmaCal ≥ 0.2.0 writes them into the
          // feed). Display only — a click opens the app, where the checkbox
          // lives; the bar is for noticing, not managing.
          Column {
            visible: root.taskRows.length > 0
            width: parent.width
            spacing: Style.space(6)

            PanelSeparator {
              visible: !root.empty
              foreground: root.foreground
            }

            PanelSectionHeader {
              text: "TASKS"
              foreground: root.foreground
              fontFamily: root.fontFamily
            }

            Repeater {
              model: root.taskRows

              CursorSurface {
                id: taskRow
                required property var modelData
                width: column.width
                hasCursor: false
                foreground: root.foreground
                implicitHeight: taskContent.implicitHeight + Style.spacing.rowPaddingX

                MouseArea {
                  anchors.fill: parent
                  hoverEnabled: true
                  cursorShape: Qt.PointingHandCursor
                  onClicked: root.openApp()
                }

                RowLayout {
                  id: taskContent
                  anchors.left: parent.left
                  anchors.right: parent.right
                  anchors.verticalCenter: parent.verticalCenter
                  anchors.leftMargin: Style.space(10)
                  anchors.rightMargin: Style.space(10)
                  spacing: Style.space(10)

                  Rectangle {
                    width: 3
                    height: Style.space(20)
                    radius: 1.5
                    color: taskRow.modelData.color ? taskRow.modelData.color : root.dim
                    Layout.alignment: Qt.AlignVCenter
                  }

                  Text {
                    Layout.fillWidth: true
                    text: taskRow.modelData.title
                    color: root.foreground
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.body
                    elide: Text.ElideRight
                  }

                  Text {
                    text: taskRow.modelData.label
                    color: taskRow.modelData.overdue ? root.urgent : root.dim
                    font.family: root.fontFamily
                    font.pixelSize: Style.font.caption
                    Layout.alignment: Qt.AlignVCenter
                  }
                }
              }
            }
          }

          // The tray menu's remaining vocabulary, so the tray icon is
          // dispensable: sync (s) and quit (q). Open lives on the hero.
          Column {
            width: parent.width
            spacing: Style.space(6)

            PanelSeparator {
              foreground: root.foreground
            }

            Row {
              anchors.right: parent.right
              spacing: Style.space(6)

              PanelActionButton {
                id: syncButton
                iconText: ""
                foreground: root.foreground
                fontFamily: root.fontFamily
                onClicked: root.syncNow()

                PanelToolTip {
                  visible: syncButton.containsMouse
                  text: "Sync now"
                  fontFamily: root.fontFamily
                }
              }

              PanelActionButton {
                id: quitButton
                iconText: ""
                foreground: root.foreground
                fontFamily: root.fontFamily
                onClicked: root.quitApp()

                PanelToolTip {
                  visible: quitButton.containsMouse
                  text: "Quit OmaCal"
                  fontFamily: root.fontFamily
                }
              }
            }
          }
        }
      }
    }
  }

  component EventRow: CursorSurface {
    id: row
    property var event: null
    property int flatIndex: 0
    property string sectionTitle: ""
    readonly property bool inNow: sectionTitle === "NOW"
    readonly property bool inOngoing: sectionTitle === "ONGOING"

    hasCursor: root.cursorActive && root.rowCursor === flatIndex
    foreground: root.foreground

    implicitHeight: rowContent.implicitHeight + Style.spacing.rowPaddingX

    MouseArea {
      anchors.fill: parent
      hoverEnabled: true
      cursorShape: Qt.PointingHandCursor
      onEntered: { root.cursorActive = true; root.rowCursor = row.flatIndex }
      onClicked: root.activateRow(row.event)
    }

    RowLayout {
      id: rowContent
      anchors.left: parent.left
      anchors.right: parent.right
      anchors.verticalCenter: parent.verticalCenter
      anchors.leftMargin: Style.space(10)
      anchors.rightMargin: Style.space(10)
      spacing: Style.space(10)

      // The calendar's colour as a slim leading tick, the same signal the
      // app's own grid uses for "which calendar is this".
      Rectangle {
        width: 3
        height: Style.space(28)
        radius: 1.5
        color: row.event && row.event.color ? row.event.color : root.dim
        Layout.alignment: Qt.AlignVCenter
      }

      ColumnLayout {
        Layout.fillWidth: true
        spacing: Style.space(1)

        Text {
          Layout.fillWidth: true
          text: Model.title(row.event)
          color: root.foreground
          font.family: root.fontFamily
          font.pixelSize: Style.font.body
          elide: Text.ElideRight
        }

        Text {
          Layout.fillWidth: true
          visible: text !== ""
          text: {
            var meta = Model.metaText(row.event)
            var lead = ""
            if (row.inNow) lead = Model.endsText(row.event, root.nowMs)
            else if (row.inOngoing) lead = Model.untilText(row.event)
            if (lead === "") return meta
            return meta === "" ? lead : lead + "  ·  " + meta
          }
          color: root.dim
          font.family: root.fontFamily
          font.pixelSize: Style.font.caption
          elide: Text.ElideRight
        }
      }

      Text {
        visible: !row.inOngoing
        text: row.inOngoing ? "" : Model.timeText(row.event)
        color: row.inNow ? root.urgent : root.foreground
        opacity: row.inNow ? 1.0 : 0.75
        font.family: root.fontFamily
        font.pixelSize: Style.font.bodySmall
        Layout.alignment: Qt.AlignVCenter
      }

      PanelActionButton {
        visible: row.event !== null && !!row.event.conference
        iconText: ""
        foreground: root.foreground
        fontFamily: root.fontFamily
        Layout.alignment: Qt.AlignVCenter
        onClicked: root.activateRow(row.event)

        PanelToolTip {
          visible: parent.containsMouse
          text: "Join the call"
          fontFamily: root.fontFamily
        }
      }
    }
  }
}
