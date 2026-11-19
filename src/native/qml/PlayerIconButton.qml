import QtQuick
import QtQuick.Controls.Basic

ToolButton {
    id: root
    property alias iconSource: root.icon.source
    property color foreground: "#ffffff"
    property bool active: false
    property string hint
    property int iconSize: 24
    property color hoverColor: "#2cffffff"

    implicitWidth: 44
    implicitHeight: 44
    Accessible.name: hint
    ToolTip.text: hint
    ToolTip.visible: hovered && hint.length > 0
    ToolTip.delay: 650

    display: text.length ? AbstractButton.TextOnly : AbstractButton.IconOnly
    icon.width: iconSize
    icon.height: iconSize
    readonly property color ink: active ? "#53bcff" : foreground
    icon.color: ink
    palette.buttonText: ink
    palette.windowText: ink
    font.weight: Font.DemiBold
    font.pixelSize: 16

    background: Rectangle {
        radius: 8
        color: root.down ? "#50ffffff" : (root.hovered ? root.hoverColor : "transparent")
        border.color: root.visualFocus ? "#009df7" : "transparent"
        border.width: 2
    }
}
