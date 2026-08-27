use crate::numeric;

/// MBR 分区表项（16 字节）。
///
/// 布局：
/// ```text
/// offset  size  field
/// 0       1     boot_indicator（0x80 = 活动，0x00 = 非活动）
/// 1       3     CHS 起始地址
/// 4       1     partition_type（文件系统类型）
/// 5       3     CHS 结束地址
/// 8       4     LBA 起始扇区（小端）
/// 12      4     分区扇区数（小端）
/// ```
#[repr(C, packed)]
#[derive(Debug, Clone, Copy)]
pub struct MbrPartitionEntry {
    pub boot_indicator: BootIndicator,
    pub chs_start: [u8; 3],
    pub partition_type: PartitionType,
    pub chs_end: [u8; 3],
    pub lba_start: u32,
    pub sector_count: u32,
}

/// 分区活动标志。
numeric! {
    pub enum BootIndicator : u8 {
        INACTIVE = 0x00,
        ACTIVE = 0x80,
    }
}

/// MBR 分区类型（文件系统类型）。
///
/// 取值参照 util-linux `fdisk/i386_sys_types.c` 的标准分区类型表。
numeric! {
    pub enum PartitionType : u8 {
        EMPTY = 0x00,
        FAT12 = 0x01,
        XENIX_ROOT = 0x02,
        XENIX_USR = 0x03,
        FAT16_LT_32M = 0x04,
        EXTENDED = 0x05,
        FAT16 = 0x06,
        NTFS = 0x07, // HPFS/NTFS/exFAT
        AIX = 0x08,
        AIX_BOOTABLE = 0x09,
        OS2_BOOT_MANAGER = 0x0A,
        FAT32 = 0x0B,
        FAT32_LBA = 0x0C,
        FAT16_LBA = 0x0E,
        EXTENDED_LBA = 0x0F,
        OPUS = 0x10,
        HIDDEN_FAT12 = 0x11,
        COMPAQ_DIAGNOSTICS = 0x12,
        HIDDEN_FAT16_LT_32M = 0x14,
        HIDDEN_FAT16 = 0x16,
        HIDDEN_NTFS = 0x17, // 隐藏 HPFS/NTFS
        AST_SMARTSLEEP = 0x18,
        HIDDEN_FAT32 = 0x1B,
        HIDDEN_FAT32_LBA = 0x1C,
        HIDDEN_FAT16_LBA = 0x1E,
        NEC_DOS = 0x24,
        PLAN9 = 0x39,
        PARTITIONMAGIC_RECOVERY = 0x3C,
        VENIX_80286 = 0x40,
        PPC_PREP_BOOT = 0x41,
        SFS = 0x42, // Secure File System
        QNX4 = 0x4D,
        QNX4_SECOND = 0x4E,
        QNX4_THIRD = 0x4F,
        ONTRACK_DM = 0x50,
        ONTRACK_DM6_AUX1 = 0x51,
        CPM = 0x52, // CP/M
        ONTRACK_DM6_AUX3 = 0x53,
        ONTRACK_DM6 = 0x54,
        EZ_DRIVE = 0x55,
        GOLDEN_BOW = 0x56,
        PRIAM_EDISK = 0x5C,
        SPEEDSTOR = 0x61,
        GNU_HURD = 0x63, // GNU HURD / Mach / SysV
        NETWARE_286 = 0x64,
        NETWARE_386 = 0x65,
        DISK_SECURE_MULTIBOOT = 0x70,
        PC_IX = 0x75,
        MINIX_OLD = 0x80, // Minix 1.4a 及更早
        MINIX = 0x81,     // Minix 1.4b 及更新 / 旧 Linux
        LINUX_SWAP = 0x82,
        LINUX = 0x83,
        OS2_HIDDEN_C_DRIVE = 0x84,
        LINUX_EXTENDED = 0x85,
        NTFS_VOLUME_SET = 0x86,
        NTFS_VOLUME_SET_2 = 0x87, // 亦被用作 Linux 自动挂载标记
        LINUX_PLAINTEXT = 0x88,
        LINUX_LVM = 0x8E,
        AMOEBA = 0x93,
        AMOEBA_BBT = 0x94, // Amoeba 坏块表
        BSD_OS = 0x9F,     // BSDI
        THINKPAD_HIBERNATION = 0xA0, // IBM Thinkpad 休眠分区
        FREEBSD = 0xA5,
        OPENBSD = 0xA6,
        NEXTSTEP = 0xA7,
        DARWIN_UFS = 0xA8,
        NETBSD = 0xA9,
        DARWIN_BOOT = 0xAB,
        HFS = 0xAF, // HFS / HFS+
        BSDI_FS = 0xB7,
        BSDI_SWAP = 0xB8,
        BOOT_WIZARD_HIDDEN = 0xBB,
        SOLARIS_BOOT = 0xBE,
        SOLARIS = 0xBF,
        DRDOS_FAT12 = 0xC1, // DR-DOS/secured FAT-12
        DRDOS_FAT16_LT_32M = 0xC4,
        DRDOS_FAT16 = 0xC6,
        SYRINX = 0xC7,
        NON_FS_DATA = 0xDA,
        CPM_CTOS = 0xDB, // CP/M / CTOS
        DELL_UTILITY = 0xDE,
        BOOTIT = 0xDF,
        DOS_ACCESS = 0xE1,
        DOS_RO = 0xE3,
        SPEEDSTOR_16BIT = 0xE4, // SpeedStor 16-bit FAT 扩展分区（<1024 柱面）
        BEOS_FS = 0xEB,
        GPT_PROTECTIVE = 0xEE,
        EFI_SYSTEM = 0xEF,
        LINUX_PA_RISC_BOOT = 0xF0,
        SPEEDSTOR_12BIT = 0xF1, // SpeedStor 12-bit FAT 扩展分区
        DOS_SECONDARY = 0xF2,
        SPEEDSTOR_LARGE = 0xF4,
        VMWARE_VMFS = 0xFB,
        VMWARE_VMKCORE = 0xFC,
        LINUX_RAID = 0xFD,
        LANSTEP = 0xFE, // SpeedStor >1024 柱面 / LANstep
        XENIX_BBT = 0xFF, // Xenix 坏块表
    }
}

#[repr(C, packed)]
#[derive(Debug, Copy, Clone)]
pub struct MbrSector {
    pub boot_code: [u8; 446],
    pub partitions: [MbrPartitionEntry; 4],
    pub boot_signature: u16, // 0xAA55
}
