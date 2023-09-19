/*
 * SPDX-FileCopyleftText: 2022 Bruno Gonçalves <bigbruno@gmail.com> and Rafael Ruscher <rruscher@gmail.com>
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

import QtQuick 2.1
import QtQuick.Layouts 1.0
import org.kde.plasma.plasmoid 2.0
import org.kde.plasma.core 2.0 as PlasmaCore
import org.kde.plasma.components 2.0 as PlasmaComponent

Item {
    id: root

    // Properties
    property string outputText: ''

    // Constants
    readonly property int defaultInterval: 7000
    readonly property int toggleInterval: 500

    // Data source for running executable commands
    PlasmaCore.DataSource {
        id: executable
        engine: "executable"
        connectedSources: []

        function exec(cmd) {
            if (cmd) {
                connectSource(cmd)
            }
        }

        onNewData: {
            var exitCode = data["exit code"]
            var exitStatus = data["exit status"]
            var stdout = data["stdout"]
            var stderr = data["stderr"]
            exited(sourceName, exitCode, exitStatus, stdout, stderr)
            disconnectSource(sourceName)  // Command finished
        }

        signal exited(string cmd, int exitCode, int exitStatus, string stdout, string stderr)
    }

    // Connection to the executable DataSource
    Connections {
        target: executable
        onExited: {
            outputText = stdout
            timer.restart()
        }
    }

    // Function to run the command
    function runCommand() {
        executable.exec('ps -x | grep "/bin/bash /usr/bin/[p]ipewire-noise-remove"')
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

    Plasmoid.icon: outputText ? 'big-noise-reduction-on' : 'big-noise-reduction-off'
    Plasmoid.preferredRepresentation: Plasmoid.compactRepresentation
    Plasmoid.status: PlasmaCore.Types.PassiveStatus

    // Function to toggle the noise reduction
    function toggle() {
        var command = outputText ? 'stop' : 'start'
        executable.exec('systemctl --user ' + command + ' noise-reduction-pipewire')
        timer.interval = toggleInterval  // Shorten the interval for quick feedback
    }

    // Compact representation of the plasmoid
    Plasmoid.compactRepresentation: PlasmaCore.IconItem {
        active: compactMouseArea.containsMouse
        source: plasmoid.icon

        MouseArea {
            id: compactMouseArea
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.LeftButton | Qt.MiddleButton
            onClicked: toggle()
        }
    }
}
