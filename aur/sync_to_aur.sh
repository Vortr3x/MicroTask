#!/usr/bin/env bash
set -euo pipefail

# Helper script to publish/sync PKGBUILD to AUR
# Requirements:
# 1. Registered account on https://aur.archlinux.org/
# 2. SSH key uploaded to your AUR account
# 3. aur SSH alias or key added to ~/.ssh/config

PKGNAME="${1:-microtask}"
AUR_REPO_URL="ssh://aur@aur.archlinux.org/${PKGNAME}.git"
TEMP_DIR=$(mktemp -d)

trap 'rm -rf "$TEMP_DIR"' EXIT

echo "==> Preparing AUR package: ${PKGNAME}"
cd "$(dirname "$0")"

# Generate fresh .SRCINFO
makepkg --printsrcinfo > .SRCINFO

echo "==> Cloning or initializing AUR repository..."
if git clone "${AUR_REPO_URL}" "${TEMP_DIR}/aur_repo" 2>/dev/null; then
    echo "==> Cloned existing AUR repository"
else
    echo "==> Initializing new AUR package repository"
    mkdir -p "${TEMP_DIR}/aur_repo"
    cd "${TEMP_DIR}/aur_repo"
    git init
    git remote add origin "${AUR_REPO_URL}"
fi

cd "$(dirname "$0")"
cp PKGBUILD .SRCINFO "${TEMP_DIR}/aur_repo/"

cd "${TEMP_DIR}/aur_repo"
git add PKGBUILD .SRCINFO
if git diff --staged --quiet; then
    echo "==> No changes detected. AUR is already up to date."
else
    VERSION=$(grep -m1 '^pkgver=' PKGBUILD | cut -d= -f2)
    RELEASE=$(grep -m1 '^pkgrel=' PKGBUILD | cut -d= -f2)
    git commit -m "Update ${PKGNAME} to ${VERSION}-${RELEASE}"
    echo "==> Pushing to ${AUR_REPO_URL}..."
    git push -u origin master
    echo "==> Successfully published ${PKGNAME} to AUR!"
fi
