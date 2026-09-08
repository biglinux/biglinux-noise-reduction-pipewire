/* SPDX-License-Identifier: GPL-2.0-or-later */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasma5support as Plasma5Support

PlasmoidItem {
    id: root
    property bool micEnabled: false
    property bool outputEnabled: false
    property bool micReady: false
    property bool outputReady: false
    property bool audioAvailable: false
    property bool busy: false
    property string actionError: ""
    property int fastPolls: 0
    property bool watcherDisabled: false
    property int watcherBackoff: 1000
    readonly property bool anyEnabled: micEnabled || outputEnabled
    readonly property bool needsAttention: !audioAvailable || (micEnabled && !micReady) || (outputEnabled && !outputReady)
    readonly property string statusCommand: "/usr/bin/biglinux-microphone-cli status"
    readonly property string openConfigCommand: "/usr/bin/biglinux-microphone"
    // Watch the directory: settings.json is replaced atomically. Only a real
    // settings event refreshes the status. Missing optional tools stop this
    // lane; other failures back off instead of spawning every 100 ms.
    readonly property string watchCommand: "sh -c 'command -v inotifywait >/dev/null 2>&1 || exit 78; d=\"${XDG_CONFIG_HOME:-$HOME/.config}/biglinux-microphone\"; test -d \"$d\" || exit 75; exec inotifywait -q -e close_write,moved_to,create,delete --format=%f \"$d\" 2>/dev/null'"

    Plasmoid.status: anyEnabled ? PlasmaCore.Types.ActiveStatus : PlasmaCore.Types.PassiveStatus
    Plasmoid.icon: needsAttention ? "dialog-warning-symbolic" : (anyEnabled ? "big-noise-reduction-on" : "big-noise-reduction-off")

    function refreshStatus() { executable.exec(statusCommand) }
    function change(key, enabled) {
        if (busy) return
        busy = true
        actionError = ""
        executable.exec("/usr/bin/biglinux-microphone-cli set " + key + (enabled ? " on" : " off"))
    }
    function openConfigurator() { executable.exec(openConfigCommand) }
    function startSettingsWatch() {
        if (!watcherDisabled && watcher.connectedSources.length === 0) watcher.connectSource(watchCommand)
    }
    function parseStatus(stdout) {
        try {
            const data = JSON.parse(stdout)
            if (typeof data.mic_enabled !== "boolean" || typeof data.output_enabled !== "boolean") throw new Error("invalid status schema")
            micEnabled = data.mic_enabled
            outputEnabled = data.output_enabled
            audioAvailable = data.audio_available === true
            micReady = data.mic_running === true
            outputReady = data.output_running === true
        } catch (error) {
            audioAvailable = false
            actionError = i18nd("biglinux-microphone", "Could not read the audio status. Open settings to check the connection.")
        }
    }

    Plasma5Support.DataSource {
        id: executable
        engine: "executable"
        connectedSources: []
        function exec(command) {
            if (connectedSources.indexOf(command) === -1) connectSource(command)
        }
        onNewData: function(sourceName, data) {
            disconnectSource(sourceName)
            if (sourceName === root.statusCommand) {
                if (data["exit code"] === 0) root.parseStatus(data["stdout"])
                else {
                    root.audioAvailable = false
                    root.actionError = i18nd("biglinux-microphone", "Could not read the audio status. Open settings to check the connection.")
                }
            } else if (sourceName !== root.openConfigCommand) {
                root.busy = false
                if (data["exit code"] !== 0) root.actionError = i18nd("biglinux-microphone", "The change could not be applied. Your previous choices have been kept where possible. Open settings for details.")
                root.fastPolls = 3
                root.refreshStatus()
            }
        }
    }
    Timer {
        interval: root.fastPolls > 0 ? 1500 : 7000
        running: true
        repeat: true
        onTriggered: {
            root.refreshStatus()
            if (root.fastPolls > 0) root.fastPolls -= 1
        }
    }
    Plasma5Support.DataSource {
        id: watcher
        engine: "executable"
        connectedSources: []
        onNewData: function(sourceName, data) {
            disconnectSource(sourceName)
            const code = data["exit code"]
            if (code === 78) { root.watcherDisabled = true; return }
            if (code === 0) {
                root.watcherBackoff = 1000
                if (String(data["stdout"]).trim() === "settings.json") {
                    root.fastPolls = 3
                    root.refreshStatus()
                }
                respawn.interval = 250
            } else {
                respawn.interval = root.watcherBackoff
                root.watcherBackoff = Math.min(root.watcherBackoff * 2, 60000)
            }
            respawn.restart()
        }
    }
    Timer { id: respawn; repeat: false; onTriggered: root.startSettingsWatch() }
    Component.onCompleted: { refreshStatus(); startSettingsWatch() }

    compactRepresentation: PlasmaComponents.ToolButton {
        icon.name: Plasmoid.icon
        Accessible.name: i18nd("biglinux-microphone", "Microphone and system sound filters")
        Accessible.description: root.needsAttention ? i18nd("biglinux-microphone", "Audio needs attention. Open the controls for details.") : i18nd("biglinux-microphone", "Open audio filter controls")
        onClicked: root.expanded = !root.expanded
        TapHandler { acceptedButtons: Qt.MiddleButton; onTapped: root.openConfigurator() }
    }
    fullRepresentation: ColumnLayout {
        Layout.preferredWidth: Kirigami.Units.gridUnit * 20
        Layout.minimumWidth: Kirigami.Units.gridUnit * 14
        spacing: Kirigami.Units.smallSpacing
        PlasmaComponents.Label {
            text: i18nd("biglinux-microphone", "Filter noise")
            font.bold: true
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
        PlasmaComponents.Label {
            visible: root.actionError.length > 0 || root.needsAttention
            text: root.actionError.length > 0 ? root.actionError : i18nd("biglinux-microphone", "Some audio filters are not ready. Open settings to check them.")
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
            Accessible.name: text
        }
        PlasmaComponents.Switch {
            text: i18nd("biglinux-microphone", "Microphone filter")
            Accessible.name: text
            Accessible.description: i18nd("biglinux-microphone", "Turning this off pauses the effects without erasing your preferences.")
            checked: root.micEnabled
            enabled: !root.busy && root.audioAvailable
            Layout.fillWidth: true
            onToggled: root.change("mic", checked)
        }
        PlasmaComponents.Label {
            text: i18nd("biglinux-microphone", "Cleans your voice for calls and recordings")
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
        PlasmaComponents.Switch {
            text: i18nd("biglinux-microphone", "System sound filter")
            Accessible.name: text
            checked: root.outputEnabled
            enabled: !root.busy && root.audioAvailable
            Layout.fillWidth: true
            onToggled: root.change("output", checked)
        }
        PlasmaComponents.Label {
            text: i18nd("biglinux-microphone", "Applies your selected effects to the sound you hear.")
            Layout.fillWidth: true
            wrapMode: Text.WordWrap
        }
        PlasmaComponents.BusyIndicator { visible: root.busy; running: root.busy; Layout.alignment: Qt.AlignHCenter }
        PlasmaComponents.Button {
            text: i18nd("biglinux-microphone", "Open settings…")
            icon.name: "preferences-desktop-sound"
            Layout.alignment: Qt.AlignTrailing
            onClicked: { root.openConfigurator(); root.expanded = false }
        }
    }
}
