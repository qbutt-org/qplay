pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls

ComboBox {
    id: root
    required property list<string> choices
    required property string selectedValue
    required property color textColor
    required property color mutedColor
    required property color optionColor
    property bool russian: false
    property bool dark: true

    readonly property bool savedChoiceUnavailable: selectedValue.length > 0
        && choices.indexOf(selectedValue) < 0
    readonly property string unavailableLabel: (russian ? qsTr("Недоступно: %1") : qsTr("Unavailable: %1"))
        .arg(selectedValue)

    signal choiceActivated(string value)

    implicitHeight: 44
    enabled: choices.length > 0
    leftPadding: 12
    rightPadding: 40
    model: (savedChoiceUnavailable ? [unavailableLabel] : [])
        .concat([russian ? qsTr("Авто") : qsTr("Auto")], choices)
    currentIndex: savedChoiceUnavailable ? 0
                  : selectedValue.length ? choices.indexOf(selectedValue) + 1 : 0
    onActivated: (index) => {
        if (savedChoiceUnavailable && index === 0)
            return
        const choice = index - (savedChoiceUnavailable ? 1 : 0)
        choiceActivated(choice === 0 ? "" : choices[choice - 1])
    }

    background: Rectangle {
        radius: 7
        color: root.optionColor
        border.color: root.visualFocus ? "#009df7" : "transparent"
        border.width: 2
    }
    contentItem: Label {
        text: root.displayText
        color: root.enabled ? root.textColor : root.mutedColor
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
    }
    indicator: Image {
        x: root.width - width - 13
        y: (root.height - height) / 2
        width: 18
        height: 18
        source: "qrc:/qt/qml/Qplay/Views/assets/icons/"
            + (root.dark ? "chevron-down.svg" : "chevron-down-dark.svg")
        sourceSize: Qt.size(width, height)
        opacity: root.enabled ? 1 : 0.5
    }
    delegate: ItemDelegate {
        id: option
        required property string modelData
        required property int index
        objectName: root.objectName + "Option" + index
        width: root.width - 8
        implicitHeight: 40
        enabled: !root.savedChoiceUnavailable || index !== 0
        text: modelData
        contentItem: Label {
            text: option.text
            color: root.textColor
            verticalAlignment: Text.AlignVCenter
            elide: Text.ElideRight
        }
        background: Rectangle {
            radius: 5
            color: root.highlightedIndex === option.index
                   ? (root.dark ? "#454b51" : "#dce8f3") : "transparent"
        }
    }
    popup: Popup {
        parent: root.Overlay.overlay
        x: root.mapToItem(parent, 0, 0).x
        y: root.mapToItem(parent, 0, 0).y - implicitHeight - 4
        width: root.width
        implicitHeight: Math.min(contentItem.implicitHeight + 8, 260)
        padding: 4
        contentItem: ListView {
            clip: true
            implicitHeight: contentHeight
            model: root.delegateModel
            currentIndex: root.highlightedIndex
            ScrollIndicator.vertical: ScrollIndicator { }
        }
        background: Rectangle {
            radius: 7
            color: root.dark ? "#303336" : "#f4f6f9"
            border.color: root.dark ? "#5b6065" : "#cbd5e0"
        }
    }
}
