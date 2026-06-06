/*
 * SPDX-FileCopyleftText: 2022-2026 Bruno Goncalves <bigbruno@gmail.com>
 *                                  and Rafael Ruscher <rruscher@gmail.com>
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
import QtQuick
import QtQuick.Layouts
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.plasma.components as PlasmaComponents
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasma5support as Plasma5Support

PlasmoidItem {
    id: root

    // ── Status the icon + popup react to ─────────────────────────────
    property bool micEnabled: false
    property bool outputEnabled: false

    readonly property bool anyEnabled: micEnabled || outputEnabled

    // ── Polling cadence ──────────────────────────────────────────────
    readonly property int defaultInterval: 7000   // 7 s — idle
    readonly property int toggleInterval: 1500    // 1.5 s — settle right after a toggle

    // ── External commands the plasmoid issues ────────────────────────
    readonly property string statusCommand: "/usr/bin/biglinux-microphone-cli status"
    readonly property string toggleMicCommand: "/usr/bin/biglinux-microphone-cli toggle-mic"
    readonly property string toggleOutputCommand: "/usr/bin/biglinux-microphone-cli toggle-output"
    readonly property string openConfigCommand: "/usr/bin/biglinux-microphone"

    // Block one inotifywait event per CLI write so our own toggle does
    // not bounce back through the file watcher. Decremented when the
    // watcher emits.
    property int suppressFileEvents: 0
    // Watches the GTK app's settings file. `inotify-tools` is an
    // optional dependency — if absent, the spawn fails harmlessly and
    // the polling Timer remains the only refresh path.
    readonly property string watchCommand:
        "sh -c 'inotifywait -q -e modify,close_write,move_self --format=. " +
        "\"${HOME}/.config/biglinux-microphone/settings.json\" 2>/dev/null'"

    function refreshStatus() {
        executable.exec(statusCommand)
    }
    function toggleMic() {
        suppressFileEvents += 1
        executable.exec(toggleMicCommand)
        timer.interval = toggleInterval
    }
    function toggleOutput() {
        suppressFileEvents += 1
        executable.exec(toggleOutputCommand)
        timer.interval = toggleInterval
    }
    function startSettingsWatch() {
        if (watcher.connectedSources.length === 0) {
            watcher.connectSource(watchCommand)
        }
    }
    function openConfigurator() {
        executable.exec(openConfigCommand)
    }

    Plasmoid.status: PlasmaCore.Types.PassiveStatus
    Plasmoid.icon: anyEnabled ? "big-noise-reduction-on" : "big-noise-reduction-off"

    // No `Plasmoid.contextualActions` here on purpose: Plasma 6 already
    // injects a "Configure {Plasmoid.title}…" entry into the tray
    // right-click menu, and the popup Button below covers the same
    // shortcut. Adding our own custom action duplicates the gear icon.
    Component.onCompleted: {
        var configureAction = Plasmoid.internalAction("configure")
        if (configureAction) {
            configureAction.visible = false
        }
    }

    // ── Subprocess plumbing ──────────────────────────────────────────
    Plasma5Support.DataSource {
        id: executable
        engine: "executable"
        connectedSources: []

        signal exited(string sourceName, int exitCode, int exitStatus, string stdout, string stderr)

        function exec(cmd) {
            connectSource(cmd)
        }

        onNewData: function(sourceName, data) {
            exited(sourceName,
                   data["exit code"],
                   data["exit status"],
                   data["stdout"],
                   data["stderr"])
            disconnectSource(sourceName)
        }
    }

    Connections {
        target: executable

        function onExited(sourceName, exitCode, exitStatus, stdout, stderr) {
            if (sourceName === root.statusCommand) {
                root.parseStatus(stdout)
            } else {
                // It was a toggle (or the configurator launch). Force an
                // immediate status refresh so the icon flips without waiting
                // for the slow polling interval.
                root.refreshStatus()
            }
            timer.restart()
        }
    }

    // Lightweight JSON parse: status output is a single flat object with
    // boolean values, so we don't need a full JSON.parse — but using it
    // keeps us forward-compatible if the schema grows.
    function parseStatus(stdout) {
        try {
            var obj = JSON.parse(stdout)
            micEnabled = !!obj.mic_enabled
            outputEnabled = !!obj.output_enabled
        } catch (e) {
            console.warn("micnoise: could not parse status output:", e, stdout)
        }
    }

    Timer {
        id: timer
        interval: defaultInterval
        repeat: true
        running: true
        onTriggered: refreshStatus()
        Component.onCompleted: {
            refreshStatus()
            startSettingsWatch()
        }
    }

    // ── Instant push from GTK app / CLI / manual edits ───────────────
    // `inotifywait` blocks until the settings file changes, then exits.
    // When it exits we refresh and respawn it — exactly one round-trip
    // per external write, no polling between events. Falls back
    // gracefully to the 7 s Timer above when inotify-tools is missing.
    Plasma5Support.DataSource {
        id: watcher
        engine: "executable"
        connectedSources: []

        onNewData: function(sourceName, data) {
            disconnectSource(sourceName)
            // Skip exactly one event right after we ourselves toggled,
            // so the CLI write we just triggered doesn't echo back.
            if (suppressFileEvents > 0) {
                suppressFileEvents -= 1
            } else {
                refreshStatus()
            }
            settingsWatchRespawn.restart()
        }
    }
    // Respawn the watcher off the signal handler so connectSource()
    // doesn't reenter the engine while it's still tearing down the
    // previous source.
    Timer {
        id: settingsWatchRespawn
        interval: 100
        repeat: false
        onTriggered: startSettingsWatch()
    }

    // ── Compact (tray icon) ──────────────────────────────────────────
    compactRepresentation: Kirigami.Icon {
        source: root.anyEnabled ? "big-noise-reduction-on" : "big-noise-reduction-off"
        active: mouseArea.containsMouse

        MouseArea {
            id: mouseArea
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
            onClicked: function(mouse) {
                if (mouse.button === Qt.MiddleButton) {
                    root.openConfigurator()
                } else {
                    root.expanded = !root.expanded
                }
            }
        }
    }

    // ── Full (popup) ────────────────────────────────────────────────
    fullRepresentation: ColumnLayout {
        Layout.preferredWidth: Kirigami.Units.gridUnit * 18
        Layout.preferredHeight: Kirigami.Units.gridUnit * 9
        spacing: Kirigami.Units.smallSpacing

        PlasmaComponents.Label {
            Layout.fillWidth: true
            Layout.margins: Kirigami.Units.smallSpacing
            text: i18nd("biglinux-noise-reduction-pipewire","Filter noise")
            font.bold: true
            elide: Text.ElideRight
        }

        // Mic toggle row
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: "audio-input-microphone-symbolic"
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                PlasmaComponents.Label {
                    text: i18nd("biglinux-noise-reduction-pipewire","Microphone filter")
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                PlasmaComponents.Label {
                    text: i18nd("biglinux-noise-reduction-pipewire","Cleans your voice for calls and recordings")
                    opacity: 0.7
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                    wrapMode: Text.WordWrap
                }
            }
            PlasmaComponents.Switch {
                checked: root.micEnabled
                onToggled: root.toggleMic()
            }
        }

        // Output toggle row
        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Kirigami.Units.smallSpacing
            Layout.rightMargin: Kirigami.Units.smallSpacing
            spacing: Kirigami.Units.smallSpacing

            Kirigami.Icon {
                source: "audio-headphones-symbolic"
                Layout.preferredWidth: Kirigami.Units.iconSizes.medium
                Layout.preferredHeight: Kirigami.Units.iconSizes.medium
            }
            ColumnLayout {
                Layout.fillWidth: true
                spacing: 0
                PlasmaComponents.Label {
                    text: i18nd("biglinux-noise-reduction-pipewire","System sound filter")
                    elide: Text.ElideRight
                    Layout.fillWidth: true
                }
                PlasmaComponents.Label {
                    text: i18nd("biglinux-noise-reduction-pipewire","Cleans every sound the system plays before it reaches your speakers")
                    opacity: 0.7
                    font.pixelSize: Kirigami.Theme.smallFont.pixelSize
                    elide: Text.ElideRight
                    wrapMode: Text.WordWrap
                    Layout.fillWidth: true
                }
            }
            PlasmaComponents.Switch {
                checked: root.outputEnabled
                onToggled: root.toggleOutput()
            }
        }

        Item { Layout.fillHeight: true }

        PlasmaComponents.Button {
            Layout.alignment: Qt.AlignRight
            Layout.margins: Kirigami.Units.smallSpacing
            text: i18nd("biglinux-noise-reduction-pipewire","Open settings…")
            icon.name: "preferences-desktop-sound"
            onClicked: {
                root.openConfigurator()
                root.expanded = false
            }
        }
    }
}
