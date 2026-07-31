use core::fmt::{Display, Formatter};

use crate::{array_struct, bits, numeric};

#[repr(C)]
#[derive(Debug)]
pub struct Elf64Ehdr {
    pub e_ident: EIdent,
    pub e_type: EType,
    pub e_machine: EMachine,
    pub e_version: u32,
    pub e_entry: u64,
    pub e_phoff: u64,
    pub e_shoff: u64,
    pub e_flags: u32,
    pub e_ehsize: u16,
    pub e_phentsize: u16,
    pub e_phnum: u16,
    pub e_shentsize: u16,
    pub e_shnum: u16,
    pub e_shstrndx: u16,
}

impl<T: AsRef<[u8]>> From<T> for Elf64Ehdr {
    fn from(value: T) -> Self {
        let ptr = value.as_ref().as_ptr() as *const Elf64Ehdr;
        unsafe { ptr.read() }
    }
}

numeric! {
    pub enum EType : u16 {
        NONE = 0,
        REL = 1,
        EXEC = 2,
        DYN = 3,
        CORE = 4,
        LOOS = 0xfe00,
        HIOS = 0xfeff,
        LOPROC = 0xff00,
        HIPROC = 0xffff,
    }
}

numeric! {
    pub enum EMachine : u16 {
        NONE          = 0x0000,
        M32           = 0x0001,
        SPARC         = 0x0002,
        INTEL386      = 0x0003,
        MOTOROLA68K   = 0x0004,
        MOTOROLA88K   = 0x0005,
        IAMCU         = 0x0006,
        INTEL860      = 0x0007,
        MIPS          = 0x0008,
        S370          = 0x0009,
        MIPS_RS4_BE   = 0x000a,
        PARISC        = 0x000f,
        VPP500        = 0x0011,
        SPARC32PLUS   = 0x0012,
        INTEL960      = 0x0013,
        PPC           = 0x0014,
        PPC64         = 0x0015,
        S390          = 0x0016,
        SPU           = 0x0017,
        V800          = 0x0024,
        FR20          = 0x0025,
        RH32          = 0x0026,
        RCE           = 0x0027,
        ARM           = 0x0028,
        ALPHA         = 0x0029,
        SH            = 0x002A,
        SPARCV9       = 0x002B,
        TRICORE       = 0x002C,
        ARC           = 0x002D,
        H8_300        = 0x002E,
        H8_300H       = 0x002F,
        H8S           = 0x0030,
        H8_500        = 0x0031,
        IA_64         = 0x0032,
        MIPS_X        = 0x0033,
        COLDFIRE      = 0x0034,
        MOTOROLA68HC12        = 0x0035,
        MMA           = 0x0036,
        PCP           = 0x0037,
        NCPU          = 0x0038,
        NDR1          = 0x0039,
        STARCORE      = 0x003A,
        ME16          = 0x003B,
        ST100         = 0x003C,
        TINYJ         = 0x003D,
        X86_64        = 0x003E,
        PDSP          = 0x003F,
        PDP10         = 0x0040,
        PDP11         = 0x0041,
        FX66          = 0x0042,
        ST9PLUS       = 0x0043,
        ST7           = 0x0044,
        MOTOROLA68HC16        = 0x0045,
        MOTOROLA68HC11        = 0x0046,
        MOTOROLA68HC08        = 0x0047,
        MOTOROLA68HC05        = 0x0048,
        SVX           = 0x0049,
        ST19          = 0x004A,
        VAX           = 0x004B,
        CRIS          = 0x004C,
        JAVELIN       = 0x004D,
        FIREPATH      = 0x004E,
        ZSP           = 0x004F,
        MMIX          = 0x0050,
        HUANY         = 0x0051,
        PRISM         = 0x0052,
        AVR           = 0x0053,
        FR30          = 0x0054,
        D10V          = 0x0055,
        D30V          = 0x0056,
        V850          = 0x0057,
        M32R          = 0x0058,
        MN10300       = 0x0059,
        MN10200       = 0x005A,
        PJ            = 0x005B,
        OPENRISC      = 0x005C,
        ARC_COMPACT   = 0x005D,
        XTENSA        = 0x005E,
        VIDEOCORE     = 0x005F,
        TMM_GPP       = 0x0060,
        NS32K         = 0x0061,
        TPC           = 0x0062,
        SNP1K         = 0x0063,
        ST200         = 0x0064,
        IP2K          = 0x0065,
        MAX           = 0x0066,
        CR            = 0x0067,
        F2MC16        = 0x0068,
        MSP430        = 0x0069,
        BLACKFIN      = 0x006A,
        SE_C33        = 0x006B,
        SEP           = 0x006C,
        ARCA          = 0x006D,
        UNICORE       = 0x006E,
        EXCESS        = 0x006F,
        DXP           = 0x0070,
        ALTERA_NIOS2  = 0x0071,
        CRX           = 0x0072,
        XGATE         = 0x0073,
        C166          = 0x0074,
        M16C          = 0x0075,
        DSPIC30F      = 0x0076,
        CE            = 0x0077,
        M32C          = 0x0078,
        TSK3000       = 0x0083,
        RS08          = 0x0084,
        SHARC         = 0x0085,
        ECOG2         = 0x0086,
        SCORE7        = 0x0087,
        DSP24         = 0x0088,
        VIDEOCORE3    = 0x0089,
        LATTICEMICO32 = 0x008A,
        SE_C17        = 0x008B,
        TI_C6000      = 0x008C,
        TI_C2000      = 0x008D,
        TI_C5500      = 0x008E,
        TI_ARP32      = 0x008F,
        TI_PRU        = 0x0090,
        MMDSP_PLUS    = 0x00A0,
        CYPRESS_M8C   = 0x00A1,
        R32C          = 0x00A2,
        TRIMEDIA      = 0x00A3,
        QDSP6         = 0x00A4,
        INTEL8051     = 0x00A5,
        STXP7X        = 0x00A6,
        NDS32         = 0x00A7,
        ECOG1         = 0x00A8,
        ECOG1X        = 0x00A8,
        MAXQ30        = 0x00A9,
        XIMO16        = 0x00AA,
        MANIK         = 0x00AB,
        CRAYNV2       = 0x00AC,
        RX            = 0x00AD,
        METAG         = 0x00AE,
        MCST_ELBRUS   = 0x00AF,
        ECOG16        = 0x00B0,
        CR16          = 0x00B1,
        ETPU          = 0x00B2,
        SLE9X         = 0x00B3,
        L10M          = 0x00B4,
        K10M          = 0x00B5,
        AARCH64       = 0x00B7,
        AVR32         = 0x00B9,
        STM8          = 0x00BA,
        TILE64        = 0x00BB,
        TILEPRO       = 0x00BC,
        MICROBLAZE    = 0x00BD,
        CUDA          = 0x00BE,
        TILEGX        = 0x00BF,
        CLOUDSHIELD   = 0x00C0,
        COREA_1ST     = 0x00C1,
        COREA_2ND     = 0x00C2,
        ARC_COMPACT2  = 0x00C3,
        OPEN8         = 0x00C4,
        RL78          = 0x00C5,
        VIDEOCORE5    = 0x00C6,
        NEC78KOR      = 0x00C7,
        NXP56800EX    = 0x00C8,
        BA1           = 0x00C9,
        BA2           = 0x00CA,
        XCORE         = 0x00CB,
        MCHP_PIC      = 0x00CC,
        INTEL205      = 0x00CD,
        INTEL206      = 0x00CE,
        INTEL207      = 0x00CF,
        INTEL208      = 0x00D0,
        INTEL209      = 0x00D1,
        KM32          = 0x00D2,
        KMX32         = 0x00D3,
        KMX16         = 0x00D4,
        KMX8          = 0x00D5,
        KVARC         = 0x00D6,
        CDP           = 0x00D7,
        COGE          = 0x00D8,
        COOL          = 0x00D9,
        NORC          = 0x00DA,
        CSR_KALIMBA   = 0x00DB,
        Z80           = 0x00DC,
        VISIUM        = 0x00DD,
        FT32          = 0x00DE,
        MOXIE         = 0x00DF,
        AMDGPU        = 0x00E0,
        RISCV         = 0x00F3,
    }
}

numeric! {
    pub enum EVersion : u32 {
        NONE = 0,         // Invalid version
        CURRENT = 1,         // Current version
    }
}

array_struct! {
    pub struct EIdent : [u8; 16] {
        class: Class => 4,
        data: Endianness => 5,
        version => 6,
        os_abi: OsAbi => 7,
        abi_version => 8,
        pad_start => 9,
        n_index => 15,
    }
}

impl EIdent {
    pub fn is_elf(&self) -> bool {
        self.0[0] == 0x7F && self.0[1] == b'E' && self.0[2] == b'L' && self.0[3] == b'F'
    }
}

numeric! {
    pub enum Class : u8 {
        NONE = 0,            // Invalid class
        CLASS32 = 1,         // 32-bit objects
        CLASS64 = 2,         // 64-bit objects
    }
}

numeric! {
    pub enum OsAbi : u8 {
        SYS_V = 0,
        HP_UX = 1,
        NET_BSD = 2,
        LINUX = 3,
        SOLARIS = 6,
        AIX = 7,
        IRIS = 8,
        FREE_BSD = 9,
        TRU64 = 10,
        MODESTO = 11,
        OPEN_BSD = 12,
        OPEN_VMS = 13,
        NSK = 14,
    }
}

numeric! {
    pub enum Endianness: u8 {
        NONE = 0,
        LSB = 1,
        MSB = 2,
    }
}

#[repr(C)]
#[derive(Debug)]
pub struct Elf64Phdr {
    pub p_type: PType,
    pub p_flags: PFlags,
    pub p_offset: u64,
    pub p_vaddr: u64,
    pub p_paddr: u64,
    pub p_filesz: u64,
    pub p_memsz: u64,
    pub p_align: u64,
}

numeric! {
    pub enum PType : u32 {
        NULL         = 0x00,
        LOAD         = 0x01,
        DYNAMIC      = 0x02,
        INTERP       = 0x03,
        NOTE         = 0x04,
        SHLIB        = 0x05,
        PHDR         = 0x06,
        TLS          = 0x07,
        LOOS         = 0x60000000,
        HIOS         = 0x6FFFFFFF,
        GNU_EH_FRAME = Self::LOOS.0 + 0x474E550,
        GNU_STACK    = Self::LOOS.0 + 0x474E551,
        GNU_RELRO    = Self::LOOS.0 + 0x474E552,
        GNU_PROPERTY = Self::LOOS.0 + 0x474E553,
        SUNWBSS      = 0x6FFFFFFA,
        SUNWSTACK    = 0x6FFFFFFB,
        ARM_ARCHEXT  = 0x70000000,
        ARM_UNWIND   = 0x70000001,
    }
}

bits! {
    pub type PFlags : u32 {
        execute: 0,
        write: 1,
        read: 2,
    }
}

impl Display for PFlags {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        if self.read() {
            f.write_str("R")?
        }
        if self.write() {
            f.write_str("W")?
        }
        if self.execute() {
            f.write_str("X")?
        }

        f.write_str("")
    }
}
