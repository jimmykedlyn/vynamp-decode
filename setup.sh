#!/bin/sh
# Liga o hook de Gitflow neste clone.
#
# `core.hooksPath` mora em .git/config, que nao e versionado: o hook esta no
# repositorio, mas o git nao olha para ele ate alguem apontar. Este script faz
# essa ligacao e e idempotente -- rodar de novo nao custa nada.
#
# Uso: sh setup.sh
set -eu

git config core.hooksPath .githooks
git update-index --add --chmod=+x -- .githooks/pre-commit 2>/dev/null || true

echo "core.hooksPath = .githooks"
echo "Hook ativo. Commit em main/develop ou em branch fora do Gitflow sera bloqueado."
