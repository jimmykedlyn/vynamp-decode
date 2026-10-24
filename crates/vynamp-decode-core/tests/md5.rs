use vynamp_decode_core::Md5;

fn hex(d: [u8; 16]) -> String {
    d.iter().map(|b| format!("{b:02x}")).collect()
}

fn one_shot(data: &[u8]) -> [u8; 16] {
    let mut m = Md5::new();
    m.update(data);
    m.finish()
}

#[test]
fn rfc1321_suite() {
    let digits80 = "1234567890".repeat(8);
    let cases: [(&str, &str); 7] = [
        ("", "d41d8cd98f00b204e9800998ecf8427e"),
        ("a", "0cc175b9c0f1b6a831c399e269772661"),
        ("abc", "900150983cd24fb0d6963f7d28e17f72"),
        ("message digest", "f96b697d7cb7938d525a2f31aaf161d0"),
        (
            "abcdefghijklmnopqrstuvwxyz",
            "c3fcd3d76192e4007dfb496cca67e13b",
        ),
        (
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789",
            "d174ab98d277d9f5a5611c2c9f419d9f",
        ),
        (digits80.as_str(), "57edf4a22be3c955ac49da2e2107b67a"),
    ];
    for (input, want) in cases {
        assert_eq!(hex(one_shot(input.as_bytes())), want, "{input:?}");
    }
}

#[test]
fn padding_boundaries() {
    // Lengths around the 55/56/63/64-byte padding edges; compare streaming
    // byte-at-a-time against one-shot, and pin a few known digests.
    for len in 0..200usize {
        let data: Vec<u8> = (0..len).map(|i| (i * 7 + 3) as u8).collect();
        let mut m = Md5::new();
        for b in &data {
            m.update(std::slice::from_ref(b));
        }
        assert_eq!(m.finish(), one_shot(&data), "len {len}");
    }
    // 64 x 'a' and 1_000_000 x 'a' (well-known digests).
    assert_eq!(
        hex(one_shot(&[b'a'; 64])),
        "014842d480b571495a4a0363793f7367"
    );
    assert_eq!(
        hex(one_shot(&vec![b'a'; 1_000_000])),
        "7707d6ae4e027c70eea2a935c2296f21"
    );
}

#[test]
fn streaming_in_random_chunks_matches_one_shot() {
    let mut x = 0xDEAD_BEEFu32;
    let mut next = move || {
        x = x.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        x >> 8
    };
    let data: Vec<u8> = (0..10_000).map(|_| next() as u8).collect();
    let want = one_shot(&data);
    for _ in 0..50 {
        let mut m = Md5::new();
        let mut rest = data.as_slice();
        while !rest.is_empty() {
            let n = ((next() % 150) as usize).min(rest.len());
            let (head, tail) = rest.split_at(n);
            m.update(head);
            rest = tail;
        }
        assert_eq!(m.finish(), want);
    }
}

#[test]
fn default_equals_new() {
    assert_eq!(Md5::default().finish(), Md5::new().finish());
}
