// TG16 CRCs were cross-checked against MAME hash/tg16.xml at
// 0de3f3e47eb3cc14eecdc8728e1b4476849adbd0 (CC0-1.0); SHA-256 values are local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PceCatalogImageKind {
    Game,
    SystemCard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Turbografx16CatalogEntry {
    pub(super) crc32: u32,
    pub(super) sha256: [u8; 32],
    pub(super) kind: PceCatalogImageKind,
}

const fn hex_nibble(value: u8) -> u8 {
    match value {
        b'0'..=b'9' => value - b'0',
        b'a'..=b'f' => value - b'a' + 10,
        _ => panic!("invalid SHA-256 hex"),
    }
}

const fn decode_sha256(hex: &[u8; 64]) -> [u8; 32] {
    let mut output = [0; 32];
    let mut index = 0;
    while index < output.len() {
        output[index] = (hex_nibble(hex[index * 2]) << 4) | hex_nibble(hex[index * 2 + 1]);
        index += 1;
    }
    output
}

const fn entry(
    crc32: u32,
    sha256: &[u8; 64],
    kind: PceCatalogImageKind,
) -> Turbografx16CatalogEntry {
    Turbografx16CatalogEntry {
        crc32,
        sha256: decode_sha256(sha256),
        kind,
    }
}

pub(super) const TURBOGRAFX16_CATALOG: &[Turbografx16CatalogEntry] = &[
    entry(
        0x02C39660,
        b"8ab675036971d15538f0c63ea17d2542dcb437c6467ff3098ac083201d1d8764",
        PceCatalogImageKind::Game,
    ), // Time Cruise
    entry(
        0x086F148C,
        b"f502fd6aa604c8ac42a1808f656355994a1424dcca3ebef9d4d899565f7da99b",
        PceCatalogImageKind::Game,
    ), // Dragon Spirit
    entry(
        0x0BC0A12B,
        b"528c79f4bd86665f64ba1fd4e59bd92a3e13efbfa2a434e72f8f5d763c266a5b",
        PceCatalogImageKind::Game,
    ), // Falcon
    entry(
        0x0F1B59B4,
        b"90f3c80aef7b45b5a7e025bf422b5fb22bd33327b731581f838575c9df0ea5ae",
        PceCatalogImageKind::Game,
    ), // Klax
    entry(
        0x14250F9A,
        b"fe791199e5e4b178f25ec3ca1cf7dc1795296f3123c935c0f2684c0390ad9d36",
        PceCatalogImageKind::Game,
    ), // Bonk's Revenge
    entry(
        0x157B4492,
        b"9ad122903ec34cb8e62ba1aae1cfddbfcb06657d0fba55d6d0b1d280f88e30f9",
        PceCatalogImageKind::Game,
    ), // Devil's Crush
    entry(
        0x15EE889A,
        b"373de7a245a252c2f3d55dbed68d480b9098fcac6cc69be2d2c88f5b4c7a0a3e",
        PceCatalogImageKind::Game,
    ), // Champions Forever Boxing
    entry(
        0x16B40B44,
        b"1bf44c8ccbebdbd2f80ce3a318cc0e2140c934e122b3460e6a530623349e0489",
        PceCatalogImageKind::Game,
    ), // Deep Blue
    entry(
        0x220EBF91,
        b"cbde8b2b16cd52c6043f58cb47d6625480fd7ab22b34abd61fb63b91e5004cf1",
        PceCatalogImageKind::Game,
    ), // The Legendary Axe II
    entry(
        0x245040B3,
        b"9e083426ffa62a9f7577a6540a48b7f07de3f8f17e701c2b9a87fc6e3c8ad05f",
        PceCatalogImageKind::Game,
    ), // Super Volleyball
    entry(
        0x26408EA3,
        b"58d3ec034136b136ad390db9d5d82ae08beb3c2ca25dc1bd4e96caafadaaf7eb",
        PceCatalogImageKind::Game,
    ), // Final Lap Twin
    entry(
        0x2774462C,
        b"0e05643c12561ee00ea064986b58c07b4bfcfc1a1ef57a90e0aa4a9075ad4a8a",
        PceCatalogImageKind::Game,
    ), // Shockman
    entry(
        0x2909DEC6,
        b"3082d86586ed868e19c948b27b9271a694fc0fb30206bcc88e5c14e369dc961b",
        PceCatalogImageKind::Game,
    ), // Galaga '90
    entry(
        0x2B5B75FE,
        b"cadac2725711b3c442bcf237b02f5a5210c96f17625c35fa58f009e0ed39e4db",
        PceCatalogImageKind::SystemCard,
    ), // Super System Card 3.0
    entry(
        0x2D211007,
        b"a4c928eb305a6fdfadd245371dda8d3dc420b405f9398929ea3713e4555c4f35",
        PceCatalogImageKind::Game,
    ), // The Legendary Axe
    entry(
        0x2DB4C1FD,
        b"391ac4ca02931b25531bdc69502dcf0defe5499e302991cb41e3f14c36f9c44f",
        PceCatalogImageKind::Game,
    ), // Ghost Manor
    entry(
        0x2F2E2240,
        b"3c38cf362f1d671b3607b3dca7cb85a67c8649bfd3005a03272583d8225d7d1e",
        PceCatalogImageKind::Game,
    ), // King of Casino
    entry(
        0x37BAF6BC,
        b"1382d4a68acb30362346946fd790b7c270d0afdf99b84deb46e24ef56152d8fd",
        PceCatalogImageKind::Game,
    ), // Bloody Wolf
    entry(
        0x3C131486,
        b"0bf74f9e89676f4bfaefdcbca7d651641146ee83ad71499ebb0c503aaceb7237",
        PceCatalogImageKind::Game,
    ), // Legend of Hero Tonma
    entry(
        0x3CA7DB48,
        b"b10965284827d2b2849ff4cb962a8a4ebe33097be58fd88c2c4973d886ba98a3",
        PceCatalogImageKind::Game,
    ), // Yo, Bro
    entry(
        0x4186D0C0,
        b"6ef204d689fbb41da7c047d9b79c2ee115a55cf0344d3d64c787fc79f14d01e1",
        PceCatalogImageKind::Game,
    ), // World Class Baseball
    entry(
        0x420FA189,
        b"18d520f082517a34b420676fe94803cb38244e41e0d6c57f2f087087d4a3018d",
        PceCatalogImageKind::Game,
    ), // Ballistix
    entry(
        0x43B05EB8,
        b"de033172636f5cc214a4d365c7d643cb3ed720c7ca05e035142ac6f60704cb6a",
        PceCatalogImageKind::Game,
    ), // Space Harrier
    entry(
        0x474D7A72,
        b"a9285ae1e51f7c6462abd2ec78a946aa0f86ee7d57e2b36f6a3c8ee1279d085b",
        PceCatalogImageKind::Game,
    ), // Keith Courage in Alpha Zones
    entry(
        0x48E6FD34,
        b"6ec4f794011f0dabda5bb0a365671b43e734c158e69460581d0a4e909ac24d4a",
        PceCatalogImageKind::Game,
    ), // Tricky Kick
    entry(
        0x4A1A8C60,
        b"39fde08a056ccc10d9fdf3cd4ef79561ba21c35bc71e8a7ffe24a402df4d464f",
        PceCatalogImageKind::Game,
    ), // Double Dungeons
    entry(
        0x4AC97606,
        b"67400494a7940b1a8c2c6c3d9ea371e3a3cbd7523b2435af4e967a45caa7afa4",
        PceCatalogImageKind::Game,
    ), // Darkwing Duck
    entry(
        0x4B93F0AC,
        b"2decf3cadfd6679f0f884b05a406b9d098bf59cead5cb3378bccc063e0d49460",
        PceCatalogImageKind::Game,
    ), // World Sports Competition
    entry(
        0x4BB68B13,
        b"e9901fd864a8fc911316451331ffd2767e1299db93e58c6a00a9a8d661f8741d",
        PceCatalogImageKind::Game,
    ), // Soldier Blade
    entry(
        0x4CFB6E3E,
        b"97af9839a457f5359bc2bc1d8945f6fd5deda55373d17e40a5e1255fcd812b84",
        PceCatalogImageKind::Game,
    ), // Cyber Core
    entry(
        0x4F6E2DBD,
        b"c702a6b28a733f32eda3786cba7305b7e00ad482f4f10d889b0450516d2bc68c",
        PceCatalogImageKind::Game,
    ), // Sinistron
    entry(
        0x4FF01515,
        b"b6cceb5226c5be1e5339c95cdceef6ea4d31ecef428f24012953d8a38f44ecb8",
        PceCatalogImageKind::Game,
    ), // Dungeon Explorer
    entry(
        0x56171C1C,
        b"678a097de779f3b24fc535e1de55448b754be6abe203a060e8f43c5c78899c8e",
        PceCatalogImageKind::Game,
    ), // Bomberman '93
    entry(
        0x599EAD9B,
        b"457a14f5de9f22f23bab3d1b0e811d21c77e3617c164087271f952e6b2f8d110",
        PceCatalogImageKind::Game,
    ), // Bonk's Adventure
    entry(
        0x5A3F76D8,
        b"de40e6780d07832cbc7f4eed43267ee2241fd30f7a1bcf92346b58687fdad741",
        PceCatalogImageKind::Game,
    ), // Bonk 3
    entry(
        0x5D395019,
        b"849c520a22e6b166f34ccf7a5a56d2b1f3e9c43aa860d7df90cec7558078d3a9",
        PceCatalogImageKind::Game,
    ), // Timeball
    entry(
        0x5E25B557,
        b"c8a438359f1a65c0f0a079b2c89274760b34a78e32244f11b677eca43c5f22e4",
        PceCatalogImageKind::Game,
    ), // TV Sports Football
    entry(
        0x5F6F3C2A,
        b"b9f98ce3d8b8baf1c6d02c2c0af72f8c0c554e533f13deb8f389853fe33bbf9e",
        PceCatalogImageKind::Game,
    ), // Bomberman
    entry(
        0x605BE213,
        b"2810bf5022e23859086ba4d8ca5c9800715b41ed65b59aac233d26693fd5dd7e",
        PceCatalogImageKind::Game,
    ), // Boxyboy
    entry(
        0x6CC10824,
        b"5b9d47aac4ed3c9a898ade3db0151e9007c8d94c2aa9048e03fbe8f1323acf9b",
        PceCatalogImageKind::Game,
    ), // Psychosis
    entry(
        0x756A1802,
        b"cb3d99ea5b65f979b28d8a3c6753e9f63aa0aa44f3391e0347e0f5e0d440f85e",
        PceCatalogImageKind::Game,
    ), // New Adventure Island
    entry(
        0x77A924B7,
        b"e4cfdac3e237c8ee940bfa7c18e3bfd01644c8dfc86462687ba9c9910c899348",
        PceCatalogImageKind::Game,
    ), // Samurai-Ghost
    entry(
        0x79D49A0D,
        b"a6d0c3a9a9f0b212ff862f2a1cca21e5ffda124c037c3afbcde042bd39d49c55",
        PceCatalogImageKind::Game,
    ), // Vigilante
    entry(
        0x7D2C4B09,
        b"549741dedb9a78a1d093f32449f0e51a6fcc2c40e99baddd1b626f6fac46ea17",
        PceCatalogImageKind::Game,
    ), // Dragon's Curse
    entry(
        0x83384572,
        b"746955e845067a481c9ab1fd0c66ec48a927cc0ece3d5268993b8d15e9f553e1",
        PceCatalogImageKind::Game,
    ), // Jack Nicklaus' Turbo Golf
    entry(
        0x85CBD045,
        b"769aafaee88972586a77c2b4359ad837844a429a7e7b86ae36d25fcaf0266b20",
        PceCatalogImageKind::Game,
    ), // Victory Run
    entry(
        0x8621AE02,
        b"635856bc1bb82686ee8547ef1920a4acaa84474bf74dd73d8a1cb66ee0f83e4e",
        PceCatalogImageKind::Game,
    ), // Off the Wall prototype
    entry(
        0x8B29C3AA,
        b"efe0ec9b264d87feedf1c878fd639d7fa168a86e5fedfbd4282d22e9c82b85c8",
        PceCatalogImageKind::Game,
    ), // Hit the Ice
    entry(
        0x8CD13E9A,
        b"4766db13b679c72d70912e68da83dd02e21b0028529df95a50d3557e995d4ddd",
        PceCatalogImageKind::Game,
    ), // Chew Man Fu
    entry(
        0x8FCAF2E9,
        b"d5d1d370529aea78c1797f01a20bdc61f2786663409bb6d2d27357cd01998d29",
        PceCatalogImageKind::Game,
    ), // Somer Assault
    entry(
        0x9033E83A,
        b"ab228a16877b457a6c35934f6f95abf10cbb1ef52dd469f2bae4b52368ef7edf",
        PceCatalogImageKind::Game,
    ), // Cratermaze
    entry(
        0x91CE5156,
        b"87a443f6b8170d8fe71d470500059b4da36f8552064684f144bb6e4329bc6f4f",
        PceCatalogImageKind::Game,
    ), // R-Type
    entry(
        0x9298254C,
        b"b224e8d4feb15c77e1478cdd4f0d0c65750f7f11af8811c217e8b839e5ed7cf9",
        PceCatalogImageKind::Game,
    ), // Chase H.Q.
    entry(
        0x933D5BCC,
        b"a571109b0d6de9c8949403b0a90f79dff6e309268d84f6da8cb42754137c06ac",
        PceCatalogImageKind::Game,
    ), // Air Zonk
    entry(
        0x93F316F7,
        b"bbddcf77eaeee374f69d850b6cde686f03f94c823ba9193abab491bf56265a53",
        PceCatalogImageKind::Game,
    ), // Military Madness
    entry(
        0x95CD2979,
        b"c5a39c9d9b2d753244816eafd68f504a855908eebab1b1c8fea2bbf7a4a813c7",
        PceCatalogImageKind::Game,
    ), // Magical Chase
    entry(
        0x97FE5BCF,
        b"866b93e455a2ebe5afff9386b517e02d1feb03e72c276ad8e7e3423e48c9a061",
        PceCatalogImageKind::Game,
    ), // TV Sports Hockey
    entry(
        0x985D492D,
        b"1e3548093a4e1a67e29ae472579e54d7f057db3fd4dd0f4adb76ae27f9a77553",
        PceCatalogImageKind::Game,
    ), // Tiger Road
    entry(
        0x99D14FB7,
        b"6a4da2c82128221431cc47020ad423e25c50a24fd03ccdd7cbbd5a775f40ba37",
        PceCatalogImageKind::Game,
    ), // Veigues
    entry(
        0x9D2F6193,
        b"8e339f40a7e0fdc717b2f0be5be6e2019f4206e74f48ebaa4b7da557dee6b40b",
        PceCatalogImageKind::Game,
    ), // Jackie Chan's Action Kung Fu
    entry(
        0x9EDAB596,
        b"eb9cc11ba7de7da8b3c1204fb42b16244203b7577d687801a7ef1d18ebba4ba9",
        PceCatalogImageKind::Game,
    ), // Davis Cup Tennis
    entry(
        0xA2EE361D,
        b"3b9f459c864b1274861dd0cd9dd77928769553f69a8403cc5f4d984631ffbc36",
        PceCatalogImageKind::Game,
    ), // China Warrior
    entry(
        0xA4457DF0,
        b"60c69ee6806aa614459549633bdc728e105f8592e535fcc796c24cd6d76a6bd1",
        PceCatalogImageKind::Game,
    ), // World Court Tennis
    entry(
        0xA980E0E9,
        b"90e04d9fcd0a57ad07ba995352f0061e396e8a51c470e1f164739fb4b86139ca",
        PceCatalogImageKind::Game,
    ), // Panza Kick Boxing
    entry(
        0xA9A94E1B,
        b"f64585688d0d8b1173c67c4719e16f16f40defa7b1009c3c35063fc5f634b8b7",
        PceCatalogImageKind::Game,
    ), // Neutopia
    entry(
        0xB03E0B32,
        b"d2fe59cf24053bbbb1b5da2521a958e38a981cce9baba0af825ea5189d8408dc",
        PceCatalogImageKind::Game,
    ), // Aero Blasters
    entry(
        0xB4A1B0F6,
        b"a862c29dbed67b10f847401da4858ff4877f4f58093c4beaa230e7065a0b5005",
        PceCatalogImageKind::Game,
    ), // Blazing Lazers
    entry(
        0xBAE9CECC,
        b"f611542440d6a3302d73170a52829f248fe34dfb4a794a7f29498a623294d44c",
        PceCatalogImageKind::Game,
    ), // TaleSpin
    entry(
        0xBB0B3AEF,
        b"04d9978d41ec53e3bc346aa6ec53ec9a473851ee8ed1d7196a3ef9859a8b69ab",
        PceCatalogImageKind::Game,
    ), // Cadash
    entry(
        0xBC59C31E,
        b"0dd97f99a0ce4ff8533eb6513fc9f95bc86004286e1b47be2b9c8bc649288987",
        PceCatalogImageKind::Game,
    ), // Raiden
    entry(
        0xC159761B,
        b"77f63df136c60517bef370e352bb30b0ec0a0385cdae629ef9a74e2958e922fa",
        PceCatalogImageKind::Game,
    ), // Night Creatures
    entry(
        0xC4ED4307,
        b"a7d5b5d1bf9d28af057c921dd80d21a998a5bcd9664694c738d3fbc69dd0295a",
        PceCatalogImageKind::Game,
    ), // Neutopia II
    entry(
        0xCCA08B02,
        b"049253b3faf564d34a61ae4a2b275518af43e7de8071bf44b6e502e846dded31",
        PceCatalogImageKind::Game,
    ), // Bravoman
    entry(
        0xD00CA74F,
        b"a9424ca8a5b6b9e559575841779ab1ef57121837e085c7544d7fb6a026da2c7a",
        PceCatalogImageKind::Game,
    ), // Splatterhouse
    entry(
        0xD1993C9F,
        b"06b8ac408d5f7942165bb06b5ce0795a6283022d034ca70dc256e17ed1af87cd",
        PceCatalogImageKind::Game,
    ), // Sidearms
    entry(
        0xD6E30CCD,
        b"814b8269e340af5e468cb91a79a5abc6b581322924556c5255be86f5a4392774",
        PceCatalogImageKind::Game,
    ), // Pac-Land
    entry(
        0xDB29486F,
        b"51c774a187925c5031b8b47b001098776f699f6a7835998a4f686688def23d19",
        PceCatalogImageKind::Game,
    ), // Super Star Soldier
    entry(
        0xDE8AF1C1,
        b"d949cb57ad44937f0070e70306de1b9b48eb027ebbb9ed649111b3d329b54595",
        PceCatalogImageKind::Game,
    ), // Ninja Spirit
    entry(
        0xE01C5127,
        b"42dd8623b9a08bca0b424d963b5b5e618b59d1e454f3888a1c278c70b6ab3fc1",
        PceCatalogImageKind::Game,
    ), // J.J. & Jeff
    entry(
        0xE2470F5F,
        b"b0fd171c6077e744f38147ac2966ae67b86440cda68c83d80f897bd9cce69292",
        PceCatalogImageKind::Game,
    ), // Impossamole
    entry(
        0xE2B0D544,
        b"4fca3b7899afe2c0ce47914c236ae26d27c26555eff01d46fc2b99ed703e7e92",
        PceCatalogImageKind::Game,
    ), // Moto Roader
    entry(
        0xE6458212,
        b"144b7a2b652af5c9376abd0e6baea752bf12e832373191af596b689095409751",
        PceCatalogImageKind::Game,
    ), // Parasol Stars
    entry(
        0xE70B01AF,
        b"dc624288e426dc7663b5112494c37cc401fe00c97476a5eec6741767395057fe",
        PceCatalogImageKind::Game,
    ), // Battle Royale
    entry(
        0xE7BF2A74,
        b"97878e97d6c498da895671d49e34e2e3b0653b94a56aba639a25eb196e8ace4e",
        PceCatalogImageKind::Game,
    ), // Ordyne
    entry(
        0xE8C3573D,
        b"2aedb10b9c99c69557c17a9658a8e072474172193f9f83ee1fd45374f29f06a3",
        PceCatalogImageKind::Game,
    ), // Fantasy Zone
    entry(
        0xE9D51797,
        b"1c45ef3e86f54102458ca77a1455b46793c23f2cbf5e3e370b4460119d3e758d",
        PceCatalogImageKind::Game,
    ), // Takin' It to the Hoop
    entry(
        0xEA488494,
        b"1af27c808be34e0c2fe435c169f5169417d6362dee0e928413581a1f6126557f",
        PceCatalogImageKind::Game,
    ), // Alien Crush
    entry(
        0xEA54D653,
        b"0b622f12a9d2218ef84461f316c9ffacb020e5a552ec93dc47b9b07b1aefe7e0",
        PceCatalogImageKind::Game,
    ), // TV Sports Basketball
    entry(
        0xEB045EDF,
        b"62bc16e81374bd98f627ae18cdbaf1bc68037e42e209d88b25b230c406501a16",
        PceCatalogImageKind::Game,
    ), // Turrican
    entry(
        0xED1D3843,
        b"7548bcec72b21492d1e027fe259300efc486a31960dea38b1420a11d88c63aef",
        PceCatalogImageKind::Game,
    ), // Power Golf
    entry(
        0xF370B58E,
        b"439cd2c04f614ea6abb44e0210a512a1f4f0c0f8bd303272195ca4048a38230a",
        PceCatalogImageKind::Game,
    ), // Gunboat
    entry(
        0xF5D98B0B,
        b"71a23db8100624451e020eb7fdb9275194783ce745603016e91247cbc1e0a709",
        PceCatalogImageKind::Game,
    ), // Dead Moon
    entry(
        0xF74E5EB3,
        b"5e86106cf38989d275e05fac2c1bce37339ad51e5ad37792d00972892266bc57",
        PceCatalogImageKind::Game,
    ), // Sonic Spike
    entry(
        0xFA7E5D66,
        b"2cd020073cfd1728904a3c599ce8b9763312da8afa53d991508b3c90fe5de1d1",
        PceCatalogImageKind::Game,
    ), // Silent Debuggers
    entry(
        0xFAE0FC60,
        b"d0f353088b3edf793038d7ec2a9e693b504689827c693b553b6341b58ae03491",
        PceCatalogImageKind::Game,
    ), // Order of the Griffon
    entry(
        0xFEA27B32,
        b"82ac6b4ca829b15ffec8f7a1739b828adf42c3cf60f37dd1d113a824a7070498",
        PceCatalogImageKind::Game,
    ), // Drop.Off
    entry(
        0xFF2A5EC3,
        b"edba5be43803b180e1d64ca678c3f8bdbf07180c9e2a65a5db69ad635951e6cc",
        PceCatalogImageKind::SystemCard,
    ), // System Card 2.0
];

pub(super) fn is_turbografx16_payload(sha256: [u8; 32]) -> bool {
    TURBOGRAFX16_CATALOG
        .iter()
        .any(|entry| entry.sha256 == sha256)
}
