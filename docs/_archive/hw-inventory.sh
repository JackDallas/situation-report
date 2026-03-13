#!/bin/bash
# Hardware inventory script — requires sudo
# Run: sudo bash docs/hw-inventory.sh > docs/hw-inventory.txt 2>&1

echo "========================================="
echo "  HARDWARE INVENTORY — $(hostname)"
echo "  $(date)"
echo "========================================="

echo ""
echo "=== MOTHERBOARD ==="
dmidecode -t baseboard 2>/dev/null | grep -E 'Manufacturer|Product|Version|Serial'

echo ""
echo "=== BIOS ==="
dmidecode -t bios 2>/dev/null | grep -E 'Vendor|Version|Release Date|BIOS Revision'

echo ""
echo "=== CPU ==="
dmidecode -t processor 2>/dev/null | grep -E 'Socket|Version|Core Count|Thread Count|Max Speed|Current Speed|Family'
echo "---"
lscpu 2>/dev/null | grep -E 'Model name|Socket|Core|Thread|CPU MHz|CPU max|CPU min|Cache|Architecture'

echo ""
echo "=== MEMORY SUMMARY ==="
dmidecode -t 16 2>/dev/null | grep -E 'Maximum Capacity|Number Of Devices|Location'

echo ""
echo "=== MEMORY SLOTS (ALL) ==="
dmidecode -t 17 2>/dev/null | grep -E 'Size|Speed|Type|Locator|Manufacturer|Part Number|Serial Number|Configured|Form Factor|Bank|Rank' | grep -v 'Error'

echo ""
echo "=== MEMORY INSTALLED ==="
free -h

echo ""
echo "=== M.2 / NVME SLOTS ==="
dmidecode -t slot 2>/dev/null | while IFS= read -r line; do
    case "$line" in
        *"System Slot"*) echo ""; echo "$line" ;;
        *Designation*|*Type*|*Current*|*Length*|*Bus*) echo "$line" ;;
    esac
done

echo ""
echo "=== NVME DRIVES ==="
nvme list 2>/dev/null || echo "(nvme-cli not installed)"
echo "---"
for d in /sys/class/nvme/nvme*/; do
    [ -d "$d" ] || continue
    echo "Device: $(basename "$d")"
    echo "  Model: $(cat "${d}model" 2>/dev/null)"
    echo "  Serial: $(cat "${d}serial" 2>/dev/null)"
    echo "  FW: $(cat "${d}firmware_rev" 2>/dev/null)"
    echo "  Transport: $(cat "${d}transport" 2>/dev/null)"
done

echo ""
echo "=== SATA CONTROLLERS ==="
lspci 2>/dev/null | grep -i sata

echo ""
echo "=== SATA PORTS (12 total on X670E) ==="
for port in /sys/class/ata_port/ata*/; do
    name=$(basename "$port")
    dev=""
    for blk in /sys/class/ata_port/${name}/device/host*/target*/*/block/*/; do
        [ -d "$blk" ] && dev=$(basename "$blk")
    done
    link="/sys/class/ata_link/link${name#ata}"
    speed=$(cat "${link}/sata_spd" 2>/dev/null)
    if [ -n "$dev" ]; then
        model=$(cat "/sys/block/${dev}/device/model" 2>/dev/null | xargs)
        size=$(lsblk -dn -o SIZE "/dev/${dev}" 2>/dev/null)
        echo "${name}: ${dev} — ${model} (${size}) @ ${speed}"
    else
        echo "${name}: EMPTY"
    fi
done

echo ""
echo "=== ALL BLOCK DEVICES ==="
lsblk -o NAME,SIZE,TYPE,FSTYPE,MOUNTPOINT,MODEL,TRAN

echo ""
echo "=== PCIE SLOTS & DEVICES ==="
dmidecode -t slot 2>/dev/null

echo ""
echo "=== PCIE LINK SPEEDS ==="
echo "GPU (01:00.0):"
lspci -vvs 01:00.0 2>/dev/null | grep -i 'lnkcap\|lnksta' | head -4
echo "NVMe (02:00.0):"
lspci -vvs 02:00.0 2>/dev/null | grep -i 'lnkcap\|lnksta' | head -4

echo ""
echo "=== GPU ==="
lspci -v 2>/dev/null | grep -A12 'VGA compatible'

echo ""
echo "=== NVIDIA GPU DETAIL ==="
nvidia-smi 2>/dev/null | head -20 || echo "(nvidia-smi not available)"

echo ""
echo "=== USB CONTROLLERS ==="
lspci 2>/dev/null | grep -i usb

echo ""
echo "=== USB DEVICES ==="
lsusb 2>/dev/null

echo ""
echo "=== NETWORK ==="
lspci 2>/dev/null | grep -iE 'ethernet|network|wifi'
echo "---"
ip link show 2>/dev/null | grep -E '^[0-9]'

echo ""
echo "=== SENSORS ==="
sensors 2>/dev/null || echo "(lm-sensors not installed)"

echo ""
echo "========================================="
echo "  DONE"
echo "========================================="
