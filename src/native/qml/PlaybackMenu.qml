pragma ComponentBehavior: Bound

import QtQuick
import QtQuick.Controls.Basic
import QtQuick.Dialogs
import QtQuick.Layouts
import Qplay.Player

Popup {
    id: root
    objectName: "playbackMenu"
    required property PlayerController backend
    required property Item host
    property bool russian: false
    property string page
    property Item trigger
    readonly property string iconRoot: "qrc:/qt/qml/Qplay/Views/assets/icons/"
    readonly property string automatic: russian ? qsTr("Авто") : qsTr("Auto")
    readonly property string off: russian ? qsTr("Выкл.") : qsTr("Off")

    function showFor(button: Item, section: string) {
        if (visible && page === section) {
            close();
            return;
        }
        trigger = button;
        page = section;
        open();
    }

    parent: trigger || host
    x: trigger ? Math.min(0, host.width - trigger.mapToItem(host, 0, 0).x - width - 12) : 0
    y: -height - 8
    width: page === "speed" ? 270 : 306
    height: Math.min(menuBody.implicitHeight + 24, host.height - 132)
    padding: 12
    focus: true
    closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutsideParent
    transformOrigin: Item.BottomLeft
    enter: Transition {
        NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 115; easing.type: Easing.OutCubic }
        NumberAnimation { property: "scale"; from: 0.96; to: 1; duration: 145; easing.type: Easing.OutCubic }
    }
    exit: Transition {
        NumberAnimation { property: "opacity"; to: 0; duration: 85 }
        NumberAnimation { property: "scale"; to: 0.98; duration: 85; easing.type: Easing.InQuad }
    }
    background: Rectangle {
        radius: 10
        color: "#fa15191e"
        border.color: "#353c44"
    }

    component Option: ItemDelegate {
        id: option
        property bool current: false
        Layout.fillWidth: true
        implicitHeight: 38
        leftPadding: 10
        rightPadding: 10
        contentItem: RowLayout {
            Label {
                Layout.fillWidth: true
                text: option.text
                color: option.enabled ? "#f4f6f8" : "#888e96"
                elide: Text.ElideRight
                font.pixelSize: 14
                verticalAlignment: Text.AlignVCenter
            }
            Image {
                visible: option.current
                source: root.iconRoot + "check.svg"
                sourceSize: Qt.size(18, 18)
            }
        }
        background: Rectangle {
            radius: 5
            color: option.down ? "#39424c" : option.hovered || option.visualFocus ? "#2a3139" : "transparent"
        }
    }

    component FxChoice: ComboBox {
        id: choice
        Layout.fillWidth: true
        implicitHeight: 36
        leftPadding: 10
        rightPadding: 30
        palette.text: "#edf2f5"
        palette.buttonText: "#edf2f5"
        palette.windowText: "#edf2f5"
        palette.window: "#20262d"
        palette.base: "#20262d"
        palette.highlight: "#16405b"
        background: Rectangle {
            radius: 5
            color: choice.hovered || choice.visualFocus ? "#333c45" : "#252c33"
        }
        indicator: Image {
            x: choice.width - width - 10
            y: (choice.height - height) / 2
            source: root.iconRoot + "chevron-down.svg"
            sourceSize: Qt.size(16, 16)
        }
    }

    component Heading: Label {
        Layout.topMargin: 5
        Layout.leftMargin: 10
        Layout.bottomMargin: 3
        color: "#adb9c4"
        font.pixelSize: 13
    }

    contentItem: ScrollView {
        id: scroll
        clip: true
        ScrollBar.horizontal.policy: ScrollBar.AlwaysOff

        ColumnLayout {
            id: menuBody
            width: scroll.availableWidth
            spacing: 2

            ColumnLayout {
                visible: root.page === "audio"
                Layout.fillWidth: true
                spacing: 0
                Heading { text: root.russian ? qsTr("Аудиодорожка") : qsTr("Audio track") }
                Option {
                    text: root.automatic
                    current: root.backend.audioSelection === "auto"
                    onClicked: root.backend.selectAutomaticTrack("audio")
                }
                Repeater {
                    model: root.backend.audioTracks
                    delegate: Option {
                        required property int trackId
                        required property string label
                        required property bool selected
                        text: label || (root.russian ? qsTr("Дорожка %1") : qsTr("Track %1")).arg(trackId)
                        current: selected && root.backend.audioSelection !== "auto"
                        onClicked: root.backend.selectTrack("audio", trackId)
                    }
                }
            }

            ColumnLayout {
                visible: root.page === "subs"
                Layout.fillWidth: true
                spacing: 0
                Heading { text: root.russian ? qsTr("Субтитры") : qsTr("Subtitles") }
                Option {
                    text: root.off
                    current: root.backend.subtitleSelection === "no"
                    onClicked: root.backend.disableSubtitles()
                }
                Option {
                    text: root.automatic
                    current: root.backend.subtitleSelection === "auto"
                    onClicked: root.backend.selectAutomaticTrack("sub")
                }
                Repeater {
                    model: root.backend.subtitleTracks
                    delegate: Option {
                        required property int trackId
                        required property string label
                        required property bool selected
                        text: label || (root.russian ? qsTr("Дорожка %1") : qsTr("Track %1")).arg(trackId)
                        current: selected && root.backend.subtitleSelection !== "auto"
                        onClicked: root.backend.selectTrack("sub", trackId)
                    }
                }
                Option {
                    text: root.russian ? qsTr("Добавить файл субтитров…") : qsTr("Add subtitle file…")
                    onClicked: subtitleDialog.open()
                }
                Option {
                    objectName: "translateSubtitles"
                    text: root.backend.subtitleTarget === "ru"
                          ? (root.russian ? qsTr("Перевести на русский") : qsTr("Translate to Russian"))
                          : (root.russian ? qsTr("Перевести на английский") : qsTr("Translate to English"))
                    current: root.backend.subtitleTranslationActive
                    onClicked: root.backend.useTranslatedSubtitles()
                }
                Label {
                    Layout.fillWidth: true
                    Layout.leftMargin: 12
                    Layout.rightMargin: 12
                    visible: ["working", "bitmap", "unsupported", "unavailable", "modelUnavailable", "failed"].includes(root.backend.subtitleStatus)
                    text: {
                        switch (root.backend.subtitleStatus) {
                        case "working": return root.russian ? qsTr("Переводим…") : qsTr("Translating…")
                        case "bitmap": return root.russian ? qsTr("Нужна текстовая дорожка субтитров") : qsTr("Choose a text subtitle track")
                        case "unsupported": return root.russian ? qsTr("Эту дорожку пока нельзя перевести") : qsTr("This track cannot be translated yet")
                        case "unavailable": return root.russian ? qsTr("Codex недоступен") : qsTr("Codex is unavailable")
                        case "modelUnavailable": return root.russian ? qsTr("Выбранная модель недоступна") : qsTr("Selected model is unavailable")
                        default: return root.russian ? qsTr("Не удалось перевести субтитры") : qsTr("Could not translate subtitles")
                        }
                    }
                    color: root.backend.subtitleStatus === "working" ? "#b9bec4" : "#ff9a9a"
                    wrapMode: Text.WordWrap
                    font.pixelSize: 13
                }

                PlayerIconButton {
                    visible: root.backend.subtitleMovable && Math.abs(root.backend.subtitlePosition - 85) >= 0.1
                    iconSource: root.iconRoot + "reset.svg"
                    hint: root.russian ? qsTr("Сбросить положение субтитров") : qsTr("Reset subtitle position")
                    onClicked: root.backend.resetSubtitlePosition()
                }
            }

            ColumnLayout {
                visible: root.page === "speed"
                Layout.fillWidth: true
                spacing: 3
                Heading {
                    text: (root.russian ? qsTr("Скорость %1×") : qsTr("Speed %1×")).arg(Number(root.backend.speed.toFixed(2)))
                }
                Slider {
                    id: speedSlider
                    objectName: "speedSlider"
                    Layout.fillWidth: true
                    implicitHeight: 44
                    from: 0.25
                    to: 4
                    stepSize: 0.05
                    value: root.backend.speed
                    enabled: root.backend.engineReady
                    Accessible.name: root.russian ? qsTr("Скорость воспроизведения") : qsTr("Playback speed")
                    onMoved: root.backend.changeSpeed(Math.round(value * 20) / 20)
                    background: Item {
                        x: speedSlider.leftPadding
                        y: speedSlider.topPadding + speedSlider.availableHeight / 2 - 3
                        width: speedSlider.availableWidth
                        height: 6
                        Rectangle {
                            anchors.fill: parent
                            radius: 3
                            color: "#74777a"
                        }
                        Rectangle {
                            width: speedSlider.visualPosition * parent.width
                            height: parent.height
                            radius: 3
                            color: "#009df7"
                        }
                    }
                    handle: Rectangle {
                        x: speedSlider.leftPadding + speedSlider.visualPosition * (speedSlider.availableWidth - width)
                        y: speedSlider.topPadding + speedSlider.availableHeight / 2 - height / 2
                        width: 14
                        height: 14
                        radius: 7
                        color: "#ffffff"
                    }
                }
                RowLayout {
                    Layout.fillWidth: true
                    Label {
                        text: "0.25×"
                        color: "#b9bec4"
                    }
                    Item {
                        Layout.fillWidth: true
                    }
                    Label {
                        text: "4×"
                        color: "#b9bec4"
                    }
                }
                GridLayout {
                    Layout.fillWidth: true
                    columns: 4
                    rowSpacing: 6
                    columnSpacing: 6
                    Repeater {
                        model: [0.5, 0.75, 1, 1.25, 1.5, 2, 3]
                        delegate: Button {
                            id: preset
                            required property real modelData
                            objectName: modelData === 2 ? "speedPreset2" : ""
                            Layout.fillWidth: true
                            implicitHeight: 44
                            text: modelData + "×"
                            font.pixelSize: 14
                            enabled: root.backend.engineReady
                            checked: Math.abs(root.backend.speed - modelData) < 0.001
                            contentItem: Label {
                                text: preset.text
                                color: "#ffffff"
                                font: preset.font
                                horizontalAlignment: Text.AlignHCenter
                                verticalAlignment: Text.AlignVCenter
                            }
                            background: Rectangle {
                                radius: 7
                                color: preset.checked ? "#009df7" : "#343638"
                                border.color: preset.visualFocus ? "#009df7" : "transparent"
                                border.width: 2
                            }
                            onClicked: root.backend.changeSpeed(modelData)
                        }
                    }
                }
            }

            ColumnLayout {
                visible: root.page === "fx"
                Layout.fillWidth: true
                spacing: 5
                Heading { text: root.russian ? qsTr("Генерация кадров") : qsTr("Frame generation") }
                RowLayout {
                    Layout.fillWidth: true
                    spacing: 2
                    Repeater {
                        model: [
                            { value: "auto", label: root.automatic },
                            { value: "on", label: root.russian ? qsTr("Вкл.") : qsTr("On") },
                            { value: "off", label: root.off }
                        ]
                        delegate: Button {
                            id: mode
                            required property var modelData
                            objectName: "framegenOption" + modelData.value
                            Layout.fillWidth: true
                            implicitHeight: 36
                            enabled: root.backend.framegenSupported
                            text: modelData.label
                            highlighted: root.backend.framegenChoice === modelData.value
                            palette.buttonText: "#ebeff3"
                            palette.brightText: "#94d6ff"
                            background: Rectangle {
                                radius: 5
                                color: mode.highlighted ? "#16364a" : mode.hovered || mode.visualFocus ? "#353d45" : "#23292f"
                            }
                            onClicked: root.backend.changeFramegen(modelData.value)
                        }
                    }
                }
                Label {
                    objectName: "framegenAvailability"
                    Layout.fillWidth: true
                    Layout.leftMargin: 10
                    visible: text.length > 0
                    wrapMode: Text.Wrap
                    color: "#b8c6d1"
                    text: {
                        if (!root.backend.framegenSupported)
                            return root.russian ? qsTr("Недоступно") : qsTr("Unavailable");
                        const part = root.backend.framegenComponent;
                        switch (root.backend.framegenStatus) {
                        case "analyzing": return root.russian ? qsTr("Анализ видео…") : qsTr("Analyzing video…");
                        case "checking": return root.russian ? qsTr("Проверка компонентов…") : qsTr("Checking components…");
                        case "downloading": return (root.russian ? qsTr("Загрузка %1 — %2%") : qsTr("Downloading %1 — %2%")).arg(part).arg(root.backend.framegenProgress);
                        case "installing": return (root.russian ? qsTr("Установка %1…") : qsTr("Installing %1…")).arg(part);
                        case "waitingGpu": return root.russian ? qsTr("Генерация занята другим окном qplay") : qsTr("Another qplay window is using frame generation");
                        case "waitingInstall": return root.russian ? qsTr("Ожидание установки в другом окне…") : qsTr("Waiting for another installation…");
                        case "building":
                            if (root.backend.framegenProgress >= 0)
                                return (root.russian ? qsTr("Компиляция модели — %1% текущего этапа") : qsTr("Model compilation — %1% of this stage")).arg(root.backend.framegenProgress);
                            return root.russian ? qsTr("Компиляция модели для этого разрешения…") : qsTr("Compiling the model for this resolution…");
                        case "starting": return root.russian ? qsTr("Запуск фильтра…") : qsTr("Starting the filter…");
                        case "active": return (root.russian ? qsTr("%1 кадр/с") : qsTr("%1 fps")).arg(Number(root.backend.framegenTarget.toFixed(2)));
                        case "guarded": return root.russian ? qsTr("Недоступно для этого видео") : qsTr("Unavailable for this video");
                        default: return "";
                        }
                    }
                }
                ProgressBar {
                    Layout.fillWidth: true
                    visible: root.backend.framegenProgress >= 0
                    from: 0
                    to: 100
                    value: root.backend.framegenProgress
                }
                Label {
                    objectName: "gpuErrorMessage"
                    visible: root.backend.gpuError.length > 0
                    Layout.fillWidth: true
                    wrapMode: Text.Wrap
                    text: root.backend.gpuError === "framegenDiskSpace"
                          ? (root.russian ? qsTr("Недостаточно места на диске") : qsTr("Not enough disk space"))
                          : root.backend.gpuError
                    color: "#ffaaa4"
                }
                Heading { text: root.russian ? qsTr("Масштабирование") : qsTr("Upscaling") }
                FxChoice {
                    id: shader
                    model: ["auto", "off", "fsr", "anime4k_a", "anime4k_b", "anime4k_c", "anime4k_aa",
                            "anime4k_bb", "anime4k_ca", "anime4k_gan_2x", "anime4k_gan_3x", "anime4k_gan_4x"]
                    readonly property var labels: [root.automatic, root.off, "FSR", "Anime4K A", "Anime4K B", "Anime4K C",
                                                   "Anime4K A+A", "Anime4K B+B", "Anime4K C+A", "Anime4K GAN 2×", "Anime4K GAN 3×", "Anime4K GAN 4×"]
                    currentIndex: model.indexOf(root.backend.shaderChoice)
                    displayText: labels[currentIndex] || ""
                    delegate: ItemDelegate {
                        required property int index
                        width: shader.width
                        text: shader.labels[index]
                        highlighted: shader.highlightedIndex === index
                    }
                    onActivated: index => root.backend.changeShader(model[index])
                }
                Heading { text: root.russian ? qsTr("Тип видео") : qsTr("Video type") }
                FxChoice {
                    model: [root.automatic, root.russian ? qsTr("Аниме") : qsTr("Anime"), root.russian ? qsTr("Фильм") : qsTr("Movie")]
                    readonly property var values: ["auto", "anime", "movie"]
                    currentIndex: values.indexOf(root.backend.contentChoice)
                    onActivated: index => root.backend.changeContent(values[index])
                }
            }
        }
    }
    FileDialog {
        id: subtitleDialog
        title: root.russian ? qsTr("Добавить субтитры") : qsTr("Add subtitles")
        onAccepted: root.backend.addSubtitle(selectedFile.toString())
    }
}
