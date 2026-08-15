import QtQuick

// The OmaCal mark — three rounded bars and the dot — as theme-tintable QML.
// Geometry is `src-tauri/icons/tray.svg`'s 128-unit canvas, scaled; keeping
// the numbers identical to that file is what makes this *the* brand mark in
// the bar rather than a lookalike. Monochrome by default so it sits in the
// bar like every other quattro icon; pass `dotColor` (the mark's own orange
// is #F97316) where a brand accent is wanted, e.g. the popup hero.
Item {
  id: root

  property real iconSize: 16
  property color color: "#ffffff"
  property color dotColor: color

  readonly property real u: iconSize / 128

  width: iconSize
  height: iconSize
  implicitWidth: iconSize
  implicitHeight: iconSize

  Rectangle {
    x: 8 * root.u; y: 12 * root.u
    width: 112 * root.u; height: 26 * root.u; radius: 13 * root.u
    color: root.color
    antialiasing: true
  }
  Rectangle {
    x: 8 * root.u; y: 49 * root.u
    width: 30 * root.u; height: 30 * root.u; radius: 15 * root.u
    color: root.dotColor
    antialiasing: true
  }
  Rectangle {
    x: 46 * root.u; y: 51 * root.u
    width: 74 * root.u; height: 26 * root.u; radius: 13 * root.u
    color: root.color
    antialiasing: true
  }
  Rectangle {
    x: 8 * root.u; y: 90 * root.u
    width: 76 * root.u; height: 26 * root.u; radius: 13 * root.u
    color: root.color
    antialiasing: true
  }
}
