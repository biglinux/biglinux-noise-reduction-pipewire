/*
 * SPDX-FileCopyleftText: 2022 Bruno Gonçalves <bigbruno@gmail.com> and Rafael Ruscher <rruscher@gmail.com>
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */
import QtQuick
import org.kde.plasma.plasmoid
import org.kde.plasma.core as PlasmaCore
import org.kde.kirigami as Kirigami
import org.kde.plasma.plasma5support as Plasma5Support

PlasmoidItem {
    id: root
    property string outputText
    property bool isActive: false

    // Constants
    readonly property int defaultInterval: 7000
    readonly property int toggleInterval: 1500
    readonly property string checkCommand: 'sh -c "test -f ~/.config/pipewire/filter-chain.conf.d/source-gtcrn-smart.conf"'

    // Function to run the command
    function runCommand() {
        executable.exec(checkCommand)
    }

    // Function to toggle the noise reduction
    function toggle() {
        var command = isActive ? 'stop' : 'start'
        executable.exec('/usr/bin/pipewire-noise-remove ' + command)
        timer.interval = toggleInterval  // Shorten the interval for quick feedback
    }

    // Function to open the GTK configurator
    function openConfigurator() {
        executable.exec('big-microphone-noise-reduction')
    }

    preferredRepresentation: fullRepresentation
    // Active = in systray and Passive in notification area
    Plasmoid.status: {
        //return PlasmaCore.Types.ActiveStatus;
        return PlasmaCore.Types.PassiveStatus;
    }

    // Context menu action to open the GTK configurator
    // Uses the translated text from the built-in configure action
    Plasmoid.contextualActions: [
        PlasmaCore.Action {
            text: Plasmoid.internalAction("configure") ? Plasmoid.internalAction("configure").text : i18n("Configure...")
            icon.name: "configure"
            onTriggered: openConfigurator()
        }
    ]

    // Hide the built-in Plasma configure action to prevent duplicate entries
    Component.onCompleted: {
        var configureAction = Plasmoid.internalAction("configure")
        if (configureAction) {
            configureAction.visible = false
        }
    }

    Plasma5Support.DataSource {
        id: "executable"
        signal exited(string sourceName, int exitCode, int exitStatus, string stdout, string stderr)
        function exec(cmd) {
            connectSource(cmd);
        }

        engine: "executable"
        connectedSources: []
        onNewData: function(sourceName, data) {
            var exitCode = data["exit code"];
            var exitStatus = data["exit status"];
            var stdout = data["stdout"];
            var stderr = data["stderr"];
            exited(sourceName, exitCode, exitStatus, stdout, stderr);
            disconnectSource(sourceName);
        }
    }

    Connections {
        function onExited(sourceName, exitCode, exitStatus, stdout, stderr) {
            if (sourceName == checkCommand) {
                Qt.callLater(function() {
                    root.outputText = stdout;
                    root.isActive = (exitCode === 0);
                });
            } else {
                // Was a toggle command, force immediate check
                runCommand()
            }
            timer.restart();
        }

        target: executable
    }

    // Timer to periodically run the command
    Timer {
        id: timer
        interval: defaultInterval
        onTriggered: runCommand()
        Component.onCompleted: {
            triggered()
        }
    }

    fullRepresentation: PlasmoidItem {
        Kirigami.Icon {
            id: icon
            source: isActive ? 'big-noise-reduction-on' : 'big-noise-reduction-off'
            height: Math.min(parent.height, parent.width)
            width: Math.min(parent.height, parent.width)
            anchors.fill: parent
        }

        MouseArea {
            anchors.fill: parent
            onClicked: toggle()
        }
    }
}
