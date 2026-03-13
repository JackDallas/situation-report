#!/bin/bash
# =============================================================================
# Situation Report: Backup Directory Setup & Cron Configuration
# =============================================================================
# Creates backup directory structure and writes maintenance scripts.
# No sudo needed — host UID 1000 (dallas) matches container UID 1000 (postgres).
#
# Prerequisites:
#   - WD Blue SATA drive mounted at /run/media/system/Storage
#   - USB-C SSD mounted at /var/mnt/sitrep-cold
#
# Usage:
#   bash backend/scripts/setup_backups.sh
# =============================================================================

set -euo pipefail

SATA_MOUNT="/run/media/system/Storage"
COLD_MOUNT="/var/mnt/sitrep-cold"
BACKUP_DIR="${SATA_MOUNT}/pg-backups"
WAL_DIR="${SATA_MOUNT}/pg-wal-archive"
COLD_DIR="${COLD_MOUNT}/pg-cold"
SCRIPT_DIR="${HOME}/.local/bin"

echo "=== Situation Report Backup Setup ==="
echo ""

# Create directories (no sudo — dallas UID 1000 = container postgres UID 1000)
echo "Creating directories..."
mkdir -p "${BACKUP_DIR}" "${WAL_DIR}" "${COLD_DIR}" "${SCRIPT_DIR}"
echo "  ${BACKUP_DIR}  -- weekly pg_dump backups"
echo "  ${WAL_DIR}      -- WAL archive files"
echo "  ${COLD_DIR}     -- cold tablespace data"

# Write the weekly backup script
cat > "${SCRIPT_DIR}/backup-situationreport.sh" << 'BACKUP_SCRIPT'
#!/bin/bash
# Weekly pg_dump backup for Situation Report
set -euo pipefail

CONTAINER="situationreport-postgres-1"
BACKUP_DIR="/mnt/backups"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
KEEP_WEEKS=8

echo "$(date): Starting backup..."

docker exec "${CONTAINER}" pg_dump \
    -U sitrep \
    -d situationreport \
    -Fc \
    --no-comments \
    -f "${BACKUP_DIR}/situationreport_${TIMESTAMP}.dump"

docker exec "${CONTAINER}" pg_restore \
    --list "${BACKUP_DIR}/situationreport_${TIMESTAMP}.dump" > /dev/null 2>&1

if [ $? -eq 0 ]; then
    SIZE=$(docker exec "${CONTAINER}" du -sh "${BACKUP_DIR}/situationreport_${TIMESTAMP}.dump" | cut -f1)
    echo "$(date): Backup OK: situationreport_${TIMESTAMP}.dump (${SIZE})"
else
    echo "$(date): BACKUP VERIFICATION FAILED" >&2
    exit 1
fi

# Rotate old backups
docker exec "${CONTAINER}" bash -c \
    "ls -t ${BACKUP_DIR}/situationreport_*.dump 2>/dev/null | tail -n +$((KEEP_WEEKS + 1)) | xargs -r rm -v"

echo "$(date): Backup complete."
BACKUP_SCRIPT

chmod +x "${SCRIPT_DIR}/backup-situationreport.sh"

# Write the WAL cleanup script
cat > "${SCRIPT_DIR}/clean-wal-archive.sh" << 'WAL_SCRIPT'
#!/bin/bash
# Daily WAL archive cleanup
set -euo pipefail

WAL_DIR="/run/media/system/Storage/pg-wal-archive"
KEEP_DAYS=7

BEFORE=$(find "${WAL_DIR}" -name "0000*" -mtime +${KEEP_DAYS} 2>/dev/null | wc -l)
find "${WAL_DIR}" -name "0000*" -mtime +${KEEP_DAYS} -delete 2>/dev/null
echo "$(date): WAL cleanup: removed ${BEFORE} files older than ${KEEP_DAYS} days"
WAL_SCRIPT

chmod +x "${SCRIPT_DIR}/clean-wal-archive.sh"

# Write the Docker cleanup script
cat > "${SCRIPT_DIR}/docker-cleanup.sh" << 'DOCKER_SCRIPT'
#!/bin/bash
# Weekly Docker cleanup
set -euo pipefail
echo "$(date): Starting Docker cleanup..."
docker system prune -f --filter "until=168h"
echo "$(date): Docker cleanup complete."
docker system df
DOCKER_SCRIPT

chmod +x "${SCRIPT_DIR}/docker-cleanup.sh"

echo ""
echo "=== Setup complete ==="
echo ""
echo "Scripts written to ${SCRIPT_DIR}/"
echo ""
echo "Add these cron entries (crontab -e):"
echo ""
echo "  # Weekly pg_dump backup (Sunday 2am)"
echo "  0 2 * * 0 ${SCRIPT_DIR}/backup-situationreport.sh >> ~/logs/sitrep-backup.log 2>&1"
echo ""
echo "  # Daily WAL archive cleanup (5am)"
echo "  0 5 * * * ${SCRIPT_DIR}/clean-wal-archive.sh >> ~/logs/sitrep-wal-cleanup.log 2>&1"
echo ""
echo "  # Weekly Docker cleanup (Sunday 4am)"
echo "  0 4 * * 0 ${SCRIPT_DIR}/docker-cleanup.sh >> ~/logs/docker-cleanup.log 2>&1"
echo ""
echo "Create log dir: mkdir -p ~/logs"
