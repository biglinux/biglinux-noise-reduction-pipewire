#!/bin/bash

SERVICE="noise-reduction-pipewire"

case "$1" in
    start)
        if systemctl --user is-active --quiet "$SERVICE"; then
            echo "enabled"
            exit 0
        fi

        echo "Iniciando o serviço de redução de ruído..."
        systemctl --user enable --now "$SERVICE"

        sleep 1
        if systemctl --user is-active --quiet "$SERVICE"; then
            echo "enabled"
        else
            echo "Erro ao iniciar o serviço."
            exit 1
        fi
        ;;

    stop)
        if ! systemctl --user is-active --quiet "$SERVICE"; then
            echo "disabled"
            exit 0
        fi

        echo "Parando o serviço de redução de ruído..."
        systemctl --user disable --now "$SERVICE"

        sleep 1
        if ! systemctl --user is-active --quiet "$SERVICE"; then
            echo "disabled"
        else
            echo "Erro ao parar o serviço."
            exit 1
        fi
        ;;

    status)
        if systemctl --user is-active --quiet "$SERVICE"; then
            echo "enabled"
        else
            echo "disabled"
        fi
        ;;

    *)
        pipewire-noise-remove "$1"
        ;;
esac
