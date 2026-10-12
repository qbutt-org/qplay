import QtQuick
import QtQuick.Controls

ToolButton {
    id: root
    property url iconSource
    property string hint
    property int iconSize: 24
    property color hoverColor: "#2cffffff"

    implicitWidth: 44
    implicitHeight: 44
    Accessible.name: hint
    ToolTip.text: hint
    ToolTip.visible: hovered && hint.length > 0
    ToolTip.delay: 650

    contentItem: Item {
        Image {
            anchors.centerIn: parent
            width: root.iconSize
            height: root.iconSize
            source: root.iconSource
            sourceSize: Qt.size(root.iconSize, root.iconSize)
            fillMode: Image.PreserveAspectFit
            opacity: root.enabled ? 1 : 0.4
        }
    }

    background: Rectangle {
        radius: 8
        color: root.down ? "#50ffffff" : (root.hovered ? root.hoverColor : "transparent")
        border.color: root.visualFocus ? "#009df7" : "transparent"
        border.width: 2
    }
}
