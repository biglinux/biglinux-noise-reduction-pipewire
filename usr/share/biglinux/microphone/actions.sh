#!/bin/bash

SERVICE="noise-reduction-pipewire"

case "$1" in
    start)
        if systemctl --user is-active --quiet "$SERVICE"; then
            echo "enabled"
            exit 0
        fi

        echo "Starting the noise reduction service..."
        systemctl --user enable --now "$SERVICE"
        ;;

    stop)
        if ! systemctl --user is-active --quiet "$SERVICE"; then
            echo "disabled"
            exit 0
        fi

        echo "Stopping the noise reduction service..."
        systemctl --user disable --now "$SERVICE"
        ;;

    enable-bluetooth)
        pipewire-noise-remove enable-bluetooth-autoswitch-to-headset
        ;;

    disable-bluetooth)
        pipewire-noise-remove disable-bluetooth-autoswitch-to-headset
        ;;

    status)
        pipewire-noise-remove status
        ;;

    status-bluetooth)
        pipewire-noise-remove status-bluetooth
        ;;
    *) # Show help in english
        echo "Usage: $0 {start|stop|status|status-bluetooth}"
        echo "start: Start the noise reduction service"
        echo "stop: Stop the noise reduction service"
        echo "status: Show the status of the noise reduction service"
        echo "status-bluetooth: Show the status of the bluetooth noise reduction service"
        
        ;;
esac
