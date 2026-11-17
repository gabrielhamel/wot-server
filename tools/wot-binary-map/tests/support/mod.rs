const BASE: u64 = 0x180000000;

pub struct Fixture {
    pub bytes: Vec<u8>,
    pub registration_argument: usize,
}

fn put(bytes: &mut [u8], offset: usize, value: impl AsRef<[u8]>) {
    let value = value.as_ref();
    bytes[offset..offset + value.len()].copy_from_slice(value);
}
fn u32_at(bytes: &mut [u8], offset: usize, value: u32) {
    put(bytes, offset, value.to_le_bytes());
}
fn u64_at(bytes: &mut [u8], offset: usize, value: u64) {
    put(bytes, offset, value.to_le_bytes());
}
fn file(rva: u32) -> usize {
    match rva {
        0x1000..0x2000 => (rva - 0x1000 + 0x400) as usize,
        0x3000..0x4000 => (rva - 0x3000 + 0x1400) as usize,
        0x5000..0x6000 => (rva - 0x5000 + 0x2400) as usize,
        0x7000..0x8000 => (rva - 0x7000 + 0x3400) as usize,
        _ => panic!("bad fixture RVA"),
    }
}
fn rip(code: &mut Vec<u8>, start: u32, prefix: &[u8], target: u32) {
    code.extend(prefix);
    let next = start + code.len() as u32 + 4;
    code.extend((target as i32 - next as i32).to_le_bytes());
}
fn register(start: u32, name: u32, style: u8, length: u32, result: u32) -> Vec<u8> {
    let mut code = vec![0x48, 0x83, 0xec, 0x38];
    rip(&mut code, start, &[0x48, 0x8d, 0x05], 0x5100);
    code.extend([0x41, 0xb9]);
    code.extend(length.to_le_bytes());
    code.extend([0x41, 0xb0, style, 0x48, 0x89, 0x44, 0x24, 0x20]);
    rip(&mut code, start, &[0x48, 0x8d, 0x15], name);
    rip(&mut code, start, &[0x48, 0x8d, 0x0d], 0x5000);
    rip(&mut code, start, &[0xe8], 0x1800);
    rip(&mut code, start, &[0x48, 0x89, 0x05], result);
    code.extend([0x48, 0x83, 0xc4, 0x38, 0xc3]);
    code
}

pub fn image() -> Fixture {
    let mut bytes = vec![0; 0x4400];
    put(&mut bytes, 0, b"MZ");
    u32_at(&mut bytes, 0x3c, 0x80);
    put(&mut bytes, 0x80, b"PE\0\0");
    put(&mut bytes, 0x84, 0x8664u16.to_le_bytes());
    put(&mut bytes, 0x86, 4u16.to_le_bytes());
    put(&mut bytes, 0x94, 240u16.to_le_bytes());
    put(&mut bytes, 0x98, 0x20bu16.to_le_bytes());
    u64_at(&mut bytes, 0x98 + 24, BASE);
    u32_at(&mut bytes, 0x98 + 108, 16);
    u32_at(&mut bytes, 0x98 + 112 + 24, 0x7000);
    u32_at(&mut bytes, 0x98 + 116 + 24, 6 * 12);
    for (index, (name, rva, offset, flags)) in [
        (b".text\0\0\0", 0x1000, 0x400, 0x60000020u32),
        (b".rdata\0\0", 0x3000, 0x1400, 0x40000040),
        (b".data\0\0\0", 0x5000, 0x2400, 0xc0000040),
        (b".pdata\0\0", 0x7000, 0x3400, 0x40000040),
    ]
    .into_iter()
    .enumerate()
    {
        let at = 0x188 + index * 40;
        put(&mut bytes, at, name);
        u32_at(&mut bytes, at + 8, 0x1000);
        u32_at(&mut bytes, at + 12, rva);
        u32_at(&mut bytes, at + 16, 0x1000);
        u32_at(&mut bytes, at + 20, offset);
        u32_at(&mut bytes, at + 36, flags);
    }
    for (rva, string) in [
        (0x3100, "ClientInterface"),
        (0x3120, "createBasePlayer"),
        (0x3140, "createCellPlayer"),
        (0x3160, "entityMethod"),
    ] {
        put(&mut bytes, file(rva), string.as_bytes());
    }
    u64_at(&mut bytes, file(0x5018), BASE + 0x3100);
    let mut range = vec![0x48, 0x83, 0xec, 0x28];
    rip(&mut range, 0x1400, &[0x48, 0x8b, 0x15], 0x5208);
    rip(&mut range, 0x1400, &[0x48, 0x8d, 0x0d], 0x5000);
    range.extend([0x41, 0xb8, 1, 0, 0, 0]);
    rip(&mut range, 0x1400, &[0xe8], 0x1900);
    range.extend([0x48, 0x83, 0xc4, 0x28, 0xc3]);
    let functions = [
        (0x1100, register(0x1100, 0x3140, 0, 7, 0x5200)),
        (0x1200, register(0x1200, 0x3120, 1, 2, 0x5210)),
        (0x1300, register(0x1300, 0x3160, 2, 0, 0x5208)),
        (0x1400, range),
        (0x1800, vec![0xc3]),
        (0x1900, vec![0xc3]),
    ];
    for (index, (start, code)) in functions.iter().enumerate() {
        put(&mut bytes, file(*start), code);
        u32_at(&mut bytes, file(0x7000) + index * 12, *start);
        u32_at(
            &mut bytes,
            file(0x7004) + index * 12,
            *start + code.len() as u32,
        );
    }
    for (index, rva) in [0x1200, 0x1100, 0x1300, 0x1400].into_iter().enumerate() {
        u64_at(&mut bytes, file(0x3000) + index * 8, BASE + rva);
    }
    u64_at(&mut bytes, file(0x5100), BASE + 0x3228);
    u64_at(&mut bytes, file(0x3240), BASE + 0x1a00);
    put(
        &mut bytes,
        file(0x1a00),
        [
            0x8b, 0x41, 0x48, 0x83, 0xf8, 0, 0x75, 6, 0xb8, 15, 0, 0, 0, 0xc3, 0xe8, 0, 0, 0, 0,
            0xc3,
        ],
    );
    u64_at(&mut bytes, file(0x5300), BASE + 0x3228);
    put(&mut bytes, file(0x5310), b".?AVVehicle@BW@@\0");
    u64_at(&mut bytes, file(0x5340), BASE + 0x3228);
    put(&mut bytes, file(0x5350), b".?AVEntity@BW@@\0");
    for (rva, value) in [
        (0x3200, 1),
        (0x320c, 0x5300),
        (0x3210, 0x3280),
        (0x3214, 0x3200),
        (0x3288, 2),
        (0x328c, 0x32a0),
        (0x32a0, 0x32c0),
        (0x32a4, 0x3300),
        (0x32c0, 0x5300),
        (0x32c4, 1),
        (0x32cc, 0xffffffff),
        (0x3300, 0x5340),
        (0x3308, 16),
        (0x330c, 0xffffffff),
    ] {
        u32_at(&mut bytes, file(rva), value);
    }
    u64_at(&mut bytes, file(0x3220), BASE + 0x3200);
    u64_at(&mut bytes, file(0x3228), BASE + 0x1800);
    Fixture {
        bytes,
        registration_argument: file(0x1100) + 19,
    }
}
