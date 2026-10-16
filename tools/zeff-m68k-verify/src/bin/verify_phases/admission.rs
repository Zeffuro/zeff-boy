use super::Case;

pub(crate) fn source_hash(cohort: &str) -> Option<&'static str> {
    match cohort {
        "NOP" => Some("9cebf91f85b38304e2496c87613621cbb983d0274a083295492ca25e2331c2b2"),
        "MOVE.q" => Some("bfbe824891e79b72b7163cf40f2c338a40ce50510926cee2981ecc55fb687c13"),
        "Bcc" => Some("1f3df93507e9be800d8cced3d7f9f59d2b562e3f23b46badc23b5bb459383ed9"),
        "ADD.b" => Some("3d76bf7b85fc237be7ef2015445e91490677443d2bd4d6e6d84886989322cae5"),
        "ADD.w" => Some("bd596a3d2ed49b0e9619643dd1d85b07b05993465fde440ab011221561e629a2"),
        "ADD.l" => Some("cc19a1d614a9ce4a68e3e85a108a3d88811348a748b38950372c09bd947e711e"),
        "NEGX.l" => Some("ca59b55f388fe4d6849c4379fafb068ce2ef3d2fc6702898ed5aa63413c48d58"),
        "EXG" => Some("6d1990dc8615dd2e0cb7193255e537827902f863ba09414c5e6091287b41de87"),
        "ABCD" => Some("8d84455744247f93c3127efdd13ec706296dc9e85b8bd66ef04b60a2c5a14a1d"),
        "ASL.b" => Some("5bfda5992400671a0e54be142b625b2c26a225d11c35cc559e9eaa5c66203554"),
        "ASL.w" => Some("1f1c952d50a3419932e22633a9247b08212787be31517f6aaa294db42432b809"),
        "ROR.l" => Some("425585edcbbd9ff8af771f2cb64e4fa88da6f2060284c30e0a7b8062efd4b2ed"),
        "BCLR" => Some("2a5ce0b7e656a74c4bf4eb6cf0bf52cf00a3717b34d853cd563545047853902f"),
        "MULS" => Some("cbe138399d53590de3bdfd446d5cd13f11cd7a3753712405df2b130a62b18b2c"),
        "DIVS" => Some("7ed3590131098159d115b86b7be284750f7e410cea8d7e37a58f0f3f0a6b24ad"),
        "MOVE.b" => Some("b4400d361e6ef1f6dada70c613e95039161ad8142dfcb64b5cfa27bd5ee24959"),
        "MOVE.w" => Some("11c25e839789cb3902d12c0901b707fc5d039008424013375a03d6216efdf653"),
        "MOVE.l" => Some("5b3c5b8de96ca76bfc2b12033541f31048d12a17172f6f286f59314450e8cc15"),
        "MOVEA.w" => Some("2de2210c3d129584c292faf6d2124af1d59c2ab715ff9025f642bdf5e230c23e"),
        "MOVEA.l" => Some("3329a76915e9b78dbf8ef0db4260339ab0458c9b4bfb22aabddfcf0f534c344c"),
        "TST.b" => Some("289681ae49b0a58b412663488c288bade03748638399aeeb4ed8fdd64d17dc2d"),
        "TST.w" => Some("0d16c2cfd8112aa82fbdda4883635ba31dba5810e14288762e0744429c2c7a78"),
        "TST.l" => Some("a37cfd0f43451d39491bb8425c2fe46270e5d43cf46e5029a1fd4cd66df65541"),
        "CLR.b" => Some("7f6d569ed57b5bf3e57e422802bb256884f2290e641ca5e6279376e5ee23170c"),
        "CLR.w" => Some("e7ac8f9efb9c71f88bc3930844e5b2f27eb61635761ab9a36ada767185acdd7a"),
        "CLR.l" => Some("f41e6a53171fb720d14ca8897e4ccc5c70aff6f78c6f241da9d4ddcba6058d0f"),
        "SUB.b" => Some("3201511e0398f360a6b27fc8ef3b04e747d5298aacdfdced8d6aba6dc39782f9"),
        "SUB.w" => Some("544902833ff09af419295b4a2bba70b5e55c2380b5444fe0107b89aa03fcb181"),
        "SUB.l" => Some("564976803df46dca0db22fcbdcc8ddfbd87d10ad6dcf62ed63f7c963b13da0e8"),
        "AND.b" => Some("dcc72a8ecf74ca6efc61a743ffdc475c448d8c0b517ae44b72044f6fc62246e4"),
        "AND.w" => Some("72f0e3ff75f1fff5f2ad221d5dcae703d7aff3256e77338a1a1594174a415e43"),
        "AND.l" => Some("bde6f203725d64dcd0e09244f5c127a0323ab16037626075cc965a3782fbcb38"),
        "OR.b" => Some("62add1222e705a287e23b4385ea45cc4ea9bd26f9b7c845d0cab06effaa9a1e3"),
        "OR.w" => Some("4d741c955f7edae2a2f5f12129f05fc28342a0ad5798552b2f177affa2cfd0d8"),
        "OR.l" => Some("649a3e0bc99758e01b237d28b7452f002fde28d770200034382f9d944a8182f4"),
        "EOR.b" => Some("87bef8de9bb884665e411dd59a501aa2c10348cbe71127c332a139228df20250"),
        "EOR.w" => Some("065fdad8329184f0154e04a1b678df32eb62a260f46ae487c2f854dcd28cce9f"),
        "EOR.l" => Some("5de170b5c84f7a85f05936651fdb7b8653b5aa5b26360aaad525f94aed9a0373"),
        "CMP.b" => Some("577ea026c89e6f8c722d343cc42eb524c43d6861d205ce45abe65d9bfbb86ce5"),
        "CMP.w" => Some("628adb87b4aa54c8f050ceb5be543139204461eb56502c15abaaa68560785d38"),
        "CMP.l" => Some("fa6c930fc6ac273308a29d1a3e535dc3be4632071299a572b7da934e5cb743d8"),
        "ADDA.w" => Some("057ddac58789c8da190494a472cb6b591411d307b4d2363b31bb6ec195ee98e7"),
        "ADDA.l" => Some("a68dc7807e3705d56c9fd0615316c9fd17aed120fce74ef26fe4b796dfd0b3b2"),
        "SUBA.w" => Some("cbc43f9db2e44dd8d869345047b444773f73ed0af376723987b7faabca30d0cb"),
        "SUBA.l" => Some("788a2d5fac9563260f7771456425f484e812887a45a77b82d33cea1e70df4022"),
        "CMPA.w" => Some("f49365f9816aa621f2a00b3e1b9eb96038221b1504a4c20bf5b5a4ec61cdee2f"),
        "CMPA.l" => Some("efd3c67626c60e4ec07e6174b8f98a8b302ba712310a041e3a516154524a40b2"),
        "ORItoCCR" => Some("34a23aee7ddcfdf113e988ab71d8f093512193d421b17f3abfdf356ff3663c92"),
        "ORItoSR" => Some("783008bbc9e27ac0be3e5928ce7353dce4eb2a44e52a2d7eed660921f44796a6"),
        "ANDItoCCR" => Some("22e73b8f5c22b375d23f901856f13a9fbe463e4ae1f8a88a12dc640bc8bd878d"),
        "ANDItoSR" => Some("6636de1c0521d818ab34683b5278e60f6b89607557da631e887960ad8b3cc487"),
        "EORItoCCR" => Some("9874e4db5b0dc5b6f68b8d8de947a1a80b50ca5d9fe0fd1faf5946112d2ca91f"),
        "EORItoSR" => Some("e0eb3039e3d117e5c52e94133cb0a7594a3a7251e378db5e5a8385822afa7117"),
        "BTST" => Some("085509a4341a447a3b9c83ffd73bda15402ddb409e30e875e507a290ac32b074"),
        "BCHG" => Some("a5508b0dda62f57553e7d4d7a4cb6fd2cf1f16c27d8beb883a6978853ed3a285"),
        "BSET" => Some("90249455984d1b9fe03f85f4739a7535142653084b472fa8c17246aebdf57bab"),
        "ASR.w" => Some("3ad48e929e93d104c7df84d67e601d7f246826cc2466d2dadfdaecddcb072bc0"),
        "LSL.w" => Some("c54f1c9edf495f4c47484c69562f68823cec8ce6903d9c408ecd79663e34e5ec"),
        "LSR.w" => Some("9dd60d26081d4c8fc0b066f8a6a89b129aec0da4436ba2a514e3f98d79389b1c"),
        "ROL.w" => Some("faa8775164afa38f336a8c73aaa651b4d00074bb5dd3b7f23b8c1cefaca9e01c"),
        "ROR.w" => Some("0e21a5bd6710b7292344fc8a628461f47909117e2d02d9143868ec5b27cb96ad"),
        "ROXL.w" => Some("f7abf0f729c8d4e6554d2a96d1f40156802e5c5f639bbfd2f5a8c03c070ec6ef"),
        "ROXR.w" => Some("a21dc7cd3cb190aae08da002e4b88daa895d23cc9fd4687ad2fe40035d741629"),
        "SBCD" => Some("09954ec662dfded16bdd367792ce120b83659213a3a41ebd0b9caafdcb36fa46"),
        "NBCD" => Some("de37b0a1706048ff0bfb5894996c60d4be2a83aadfd59a12160a3a08982139d4"),
        "ADDX.b" => Some("c58e24dcb3e5ea07788d9ee98bde51305c3fd3ebdf18979b7dc95ed279fe8cea"),
        "ADDX.w" => Some("f3d1f3a1f255452fcb0b83479510b006bc777d0a7fd1092a12942b3ff4af1785"),
        "ADDX.l" => Some("9fcaee5fe1c5f3c601ba68e21f77f7f73cda56c304a990991448854e290aab2e"),
        "SUBX.b" => Some("d9a014be40dbf81806cecd5368dd36d3f1754ec722a26530bb4bf230b86d2cc6"),
        "SUBX.w" => Some("1dcd356421bb89060f77f39168c13e9ca3449154b02988b1e1f252bb16526c61"),
        "SUBX.l" => Some("a54c7d57e876476db168e6da544da6455788158cba5f5468227ec78bca0872c9"),
        "MULU" => Some("9995eb90ff8c4293c7f6440d667ef47a2e6f34d01f2389b07d38d58740ceb7e1"),
        "DIVU" => Some("91358e1e9296a0d130c08416fef2db866de6fdc6b1876e342d90ac3ef0038c6f"),
        "CHK" => Some("7daa978c27f6463aeb520195a8ae19cfdd2cfe76fdd860493fa1a6cd9879b440"),
        "BSR" => Some("5e071f3914e0495a22e7e7d61848c3193e9c3b8f27a70bfa35d083f1e66b458f"),
        "DBcc" => Some("69464a7cfc6bfdff0edb7ef4a0c8e49baa15d9809913c3261de29e76c263ca0a"),
        "Scc" => Some("5937ecdc30935cae65c9d6e2edce144eb7ea082660533d99f13df848e3648686"),
        "JMP" => Some("2d64918cd078fcb174a5bdb60a24141cecc982b89304dc8b3af68b4055feb5b4"),
        "JSR" => Some("41943a8c373c29736aa70986553f31bb03d6be5f4a70aed1ce383ec05bdd0f67"),
        "LEA" => Some("4f30ade96e6cf65c0628b940858899f1e0db2b40b014207509c9881c4b72be29"),
        "PEA" => Some("13f7db5a0c05df2b381778ccdfd228f3751d0f36a104803e5fc4d5d0545c0d88"),
        "LINK" => Some("cb1ab4e4f812c11eef19b980e52401c0ce2ea82551be26fadb9406daa2e6d550"),
        "UNLINK" => Some("ed9327770747232b80cec2c1b0c8fabdfef2f1067af78a6b21f7c20b407d78e4"),
        "RTR" => Some("05f83705c724ca94f9af80b9afab3800fee92c94c9a398a8fd774c9604727f6c"),
        "TRAP" => Some("fead4435111103b49ccdef17f60852c438d81f21f1fd9a5ffcac0c2548ad7a85"),
        "MOVEfromUSP" => Some("7dcd639e8f8619b78581940397ba5f07ad4378d879ae3b4fa90e5e2979379947"),
        "MOVEtoUSP" => Some("f43a627e07e59c5bfc83f11ef756b1c743d0249186beb7b3f12ea16b66ea7a19"),
        "MOVEfromSR" => Some("c6a416d1e93c31d1901a1296f08e7cb14e99cf8dbe5c6541878b1956751fb725"),
        "MOVEtoSR" => Some("cf7ce17aacfc6be5a9db9456bd7d7d70206c13e456691245011064e6c0a43d75"),
        "MOVEtoCCR" => Some("603ffea2c9109daa2358288934337a52976636c59d008a75a5ba71d70b304729"),
        "MOVEM.w" => Some("0856263b984602dd2ca55fa8eac7f99fd8b48862facb917813692665d68201f9"),
        "MOVEM.l" => Some("b89ca3a4acbdd0e303a41d9fccfb67184dc7706d50cbab48c116e19a5f592bdd"),
        "MOVEP.w" => Some("56e94fc662c68e7657c61d09849c8b20c986987be133b3f871aeab8956623d4c"),
        "MOVEP.l" => Some("3af21c3ebb5fb4c142df3c2dbce25a5fb651c4a804d3691ca3f3a06c6365b16a"),
        "RESET" => Some("41bd1d0431480185d2061e171298810f7f5165c3cb819f3f502ccc212bf6f768"),
        "RTE" => Some("6f2de0d6abc51528ff62498af4e61b311102300f9f4d5fb825681116b3a0249c"),
        "RTS" => Some("772f0bde0b6c82227edbcd374364f6ae07da6c5fa01ca469a368a053d57c5b63"),
        "ILLEGAL_LINEA" => Some("83cff2cf6c8f39b849d17f1e4bcf4c9ad5de1349d91599b25ff19a2c08619353"),
        "ILLEGAL_LINEF" => Some("315e707ae69aedd927611d8467755b7ac5c9edb1623bad477f13e3bbe4d7f153"),
        "ASL.l" => Some("e44efd7fd77315877bebab514f5ede5941966a37eb1ccb0e8816a6f00d113815"),
        "ASR.b" => Some("bea8aa1c533071582d1adece5733457371883898ce0bff7045e224bf5320979d"),
        "ASR.l" => Some("81c9ae3dd7be601e2d6b4ddd58b97ab42bd06cc4cdc0cadef35e7fe46fe0ac01"),
        "EXT.l" => Some("8aa149103720d7cdfd17ce1646ab381e0bf0baa41039c20e153851ff8d46b4af"),
        "EXT.w" => Some("d7210993a82ca83ba10d34171558f6f3fa2539f02b7a6a3886b1f61d716c35e9"),
        "LSL.b" => Some("f294bf09b3b08149b7a53f8e7209eef979a035c14341ffbd84d885894cce0e17"),
        "LSL.l" => Some("559f2d0bc1d729d2342a5a8ce3e8fe9a58eb47e29a8c4305ed861b7e5e7f6abb"),
        "LSR.b" => Some("3c66880461f46bcbc3edd02e89fdfe883ac324f3e9e3d872b4a5b3420ca3c241"),
        "LSR.l" => Some("2b8f9bbed2b781fa7282bb93cb834ad4f9ee737dd24bbdea9265f5460142a1a9"),
        "NEG.b" => Some("8f4e24d794d6f6d3c824398848b95e6fdfab0af7d2bcf1399d1c68a58b9bf683"),
        "NEG.l" => Some("52bbcf950c31a102340283135284798680f936789955e7918c0b55e69898e215"),
        "NEG.w" => Some("43f173304b82e4931cabc5b37744f12b617dee9d8bced8c071876a2c4e4c413e"),
        "NEGX.b" => Some("d01e2e4f9b9e453fbd43f3d4cda6b818ad1e26554cd21cc096e25f8c3b1f49c3"),
        "NEGX.w" => Some("8a85da17cd533d9444d78bb891620569b864da8c969f3a9330373e8d55a8c4d6"),
        "NOT.b" => Some("973f147aa25b58c3fc708edb240b884836ffdc472d5bc923f14b38d56d8680b9"),
        "NOT.l" => Some("1809fb0b9dc2f657c453b5881da11bb5d813dcd2d909159bc4a8d3320591c3fc"),
        "NOT.w" => Some("5ff8f1b45297c0aef0078075976ff1751c18557b1049f1c1f4d6eef2bf66445b"),
        "ROL.b" => Some("58b6b13f627a6a23d6a19188bc8cb00934fb97bab5d00cee499d2d159984ea7b"),
        "ROL.l" => Some("fe5f2199fc91ec4883bdb8de7c75dc9414a5f231d079aabe17d85ab81e74e99d"),
        "ROR.b" => Some("aabe4dfd9ff678450b75687cfb158396e4ea400eedeec527ce5af154c2e46bee"),
        "ROXL.b" => Some("b55b34221f11dcd0c840acd28ba2f7a6a45bd0c7f551b082f73457987f117c04"),
        "ROXL.l" => Some("48fa8ce9843d6ec36a3f50751fe13515bca04d0f38feddf4155639409cd3d258"),
        "ROXR.b" => Some("a144ad40461f7e9c5985a22409345884f6ce7a319fd3c6b4de9ef7188b34d747"),
        "ROXR.l" => Some("c8ea216503d02189e4cdbe66fefbd8cb7713a3815c30799d8e750f20ee802385"),
        "SWAP" => Some("c68b48eca1bbfc9c43d9fd671a5b4bff2ca85b6c4a30415271da54317608205e"),
        _ => None,
    }
}

fn binary_family(cohort: &str) -> Option<&str> {
    let (family, _) = cohort.split_once('.')?;
    matches!(
        family,
        "ADD" | "SUB" | "AND" | "OR" | "EOR" | "CMP" | "ADDA" | "SUBA" | "CMPA"
    )
    .then_some(family)
}

fn binary_cohort_matches(op: u16, cohort: &str, size: Option<u16>) -> bool {
    if matches!(op, 0x003c | 0x007c | 0x023c | 0x027c | 0x0a3c | 0x0a7c) {
        return false;
    }
    let family = binary_family(cohort).unwrap();
    let top = op >> 12;
    let opmode = (op >> 6) & 7;
    let address = matches!(opmode, 3 | 7) && matches!(top, 9 | 11 | 13);
    if family.ends_with('A') {
        let expected_top = match family {
            "ADDA" => 13,
            "SUBA" => 9,
            _ => 11,
        };
        return top == expected_top && address && size == Some(if opmode == 3 { 1 } else { 2 });
    }
    if address {
        return matches!((family, top), ("ADD", 13) | ("SUB", 9) | ("CMP", 11))
            && size == Some(if opmode == 3 { 1 } else { 2 });
    }
    if size != Some((op >> 6) & 3) || (op >> 6) & 3 == 3 {
        return false;
    }
    match family {
        "ADD" => top == 13 || op & 0xf100 == 0x5000 || op & 0xff00 == 0x0600,
        "SUB" => top == 9 || op & 0xf100 == 0x5100 || op & 0xff00 == 0x0400,
        "AND" => {
            top == 12 && (opmode <= 2 || opmode >= 4 && (op >> 3) & 7 >= 2) || op & 0xff00 == 0x0200
        }
        "OR" => {
            top == 8 && (opmode <= 2 || opmode >= 4 && (op >> 3) & 7 >= 2) || op & 0xff00 == 0x0000
        }
        "EOR" => {
            top == 11 && (4..=6).contains(&opmode) && (op >> 3) & 7 != 1 || op & 0xff00 == 0x0a00
        }
        "CMP" => top == 11 && (opmode <= 2 || op & 0xf138 == 0xb108) || op & 0xff00 == 0x0c00,
        _ => false,
    }
}

fn binary_unsupported(op: u16) -> Option<&'static str> {
    let top = op >> 12;
    let mode = (op >> 3) & 7;
    let reg = op & 7;
    let size = (op >> 6) & 3;
    let opmode = (op >> 6) & 7;
    if top == 0 {
        return (size == 3 || mode == 1 || mode == 7 && reg > 1)
            .then_some("immediate_invalid_operand_form");
    }
    if top == 5 {
        return (size == 3 || mode == 1 && size == 0 || mode == 7 && reg > 1)
            .then_some("quick_invalid_operand_form");
    }
    if matches!(opmode, 3 | 7) {
        return (mode == 7 && reg > 4).then_some("binary_invalid_source_form");
    }
    if opmode <= 2 {
        return (mode == 1 && (!matches!(top, 9 | 11 | 13) || size == 0) || mode == 7 && reg > 4)
            .then_some("binary_invalid_source_form");
    }
    if matches!(top, 9 | 11 | 13) && mode <= 1 {
        return None;
    }
    (mode == 1 || mode == 7 && reg > 1).then_some("binary_invalid_destination_form")
}

fn shift_cohort_matches(op: u16, cohort: &str, size: Option<u16>) -> bool {
    let family = cohort.split_once('.').unwrap().0;
    let register_mask = match family {
        "ASR" => 0xe000,
        "ASL" => 0xe100,
        "LSR" => 0xe008,
        "LSL" => 0xe108,
        "ROXR" => 0xe010,
        "ROXL" => 0xe110,
        "ROR" => 0xe018,
        "ROL" => 0xe118,
        _ => return false,
    };
    let memory_mask = match family {
        "ASR" => 0xe0c0,
        "ASL" => 0xe1c0,
        "LSR" => 0xe2c0,
        "LSL" => 0xe3c0,
        "ROXR" => 0xe4c0,
        "ROXL" => 0xe5c0,
        "ROR" => 0xe6c0,
        "ROL" => 0xe7c0,
        _ => return false,
    };
    if (op >> 6) & 3 == 3 {
        size == Some(1) && op & 0xffc0 == memory_mask
    } else {
        size == Some((op >> 6) & 3) && op & 0xf118 == register_mask
    }
}

pub(super) fn cohort_matches(case: &Case) -> bool {
    let op = case.initial.prefetch[0];
    let size = (op >> 6) & 3;
    let expected_size = match case.cohort.rsplit('.').next() {
        Some("b") => Some(0),
        Some("w") => Some(1),
        Some("l") => Some(2),
        _ => None,
    };
    match case.cohort.as_str() {
        "ORItoCCR" => op == 0x003c,
        "ORItoSR" => op == 0x007c,
        "ANDItoCCR" => op == 0x023c,
        "ANDItoSR" => op == 0x027c,
        "EORItoCCR" => op == 0x0a3c,
        "EORItoSR" => op == 0x0a7c,
        "MOVEM.w" | "MOVEM.l" => {
            op & 0xfb80 == 0x4880
                && (op >> 3) & 7 != 0
                && expected_size == Some(if op & 64 == 0 { 1 } else { 2 })
        }
        "MOVEP.w" | "MOVEP.l" => {
            op & 0xf138 == 0x0108 && expected_size == Some(if op & 64 == 0 { 1 } else { 2 })
        }
        "RESET" => op == 0x4e70,
        "RTE" => op == 0x4e73,
        "RTS" => op == 0x4e75,
        "ILLEGAL_LINEA" => op >> 12 == 10,
        "ILLEGAL_LINEF" => op >> 12 == 15,
        "NOP" => op == 0x4e71,
        "MOVE.q" => op & 0xf100 == 0x7000,
        "MOVE.b" => op >> 12 == 1 && (op >> 6) & 7 != 1,
        "MOVE.w" => op >> 12 == 3 && (op >> 6) & 7 != 1,
        "MOVE.l" => op >> 12 == 2 && (op >> 6) & 7 != 1,
        "MOVEA.w" => op >> 12 == 3 && (op >> 6) & 7 == 1,
        "MOVEA.l" => op >> 12 == 2 && (op >> 6) & 7 == 1,
        "Bcc" => op >> 12 == 6 && (op >> 8) & 15 != 1,
        "BSR" => op & 0xff00 == 0x6100,
        "DBcc" => op & 0xf0f8 == 0x50c8,
        "Scc" => op & 0xf0c0 == 0x50c0 && (op >> 3) & 7 != 1,
        "JMP" => op & 0xffc0 == 0x4ec0,
        "JSR" => op & 0xffc0 == 0x4e80,
        "LEA" => op & 0xf1c0 == 0x41c0,
        "PEA" => op & 0xffc0 == 0x4840,
        "LINK" => op & 0xfff8 == 0x4e50,
        "UNLINK" => op & 0xfff8 == 0x4e58,
        "RTR" => op == 0x4e77,
        "TRAP" => op & 0xfff0 == 0x4e40,
        "MOVEfromUSP" => op & 0xfff8 == 0x4e68,
        "MOVEtoUSP" => op & 0xfff8 == 0x4e60,
        "MOVEfromSR" => op & 0xffc0 == 0x40c0,
        "MOVEtoSR" => op & 0xffc0 == 0x46c0,
        "MOVEtoCCR" => op & 0xffc0 == 0x44c0,
        "ADD.b" | "ADD.w" | "ADD.l" | "SUB.b" | "SUB.w" | "SUB.l" | "AND.b" | "AND.w" | "AND.l"
        | "OR.b" | "OR.w" | "OR.l" | "EOR.b" | "EOR.w" | "EOR.l" | "CMP.b" | "CMP.w" | "CMP.l"
        | "ADDA.w" | "ADDA.l" | "SUBA.w" | "SUBA.l" | "CMPA.w" | "CMPA.l" => {
            binary_cohort_matches(op, &case.cohort, expected_size)
        }
        "TST.b" | "TST.w" | "TST.l" => op & 0xff00 == 0x4a00 && expected_size == Some(size),
        "CLR.b" | "CLR.w" | "CLR.l" => op & 0xff00 == 0x4200 && expected_size == Some(size),
        "NEG.b" | "NEG.w" | "NEG.l" => op & 0xff00 == 0x4400 && expected_size == Some(size),
        "NEGX.b" | "NEGX.w" | "NEGX.l" => op & 0xff00 == 0x4000 && expected_size == Some(size),
        "NOT.b" | "NOT.w" | "NOT.l" => op & 0xff00 == 0x4600 && expected_size == Some(size),
        "EXT.w" => op & 0xfff8 == 0x4880,
        "EXT.l" => op & 0xfff8 == 0x48c0,
        "SWAP" => op & 0xfff8 == 0x4840,
        "EXG" => matches!(op & 0xf1f8, 0xc140 | 0xc148 | 0xc188),
        "ABCD" => op & 0xf1f0 == 0xc100,
        "SBCD" => op & 0xf1f0 == 0x8100,
        "NBCD" => op & 0xffc0 == 0x4800,
        "ADDX.b" | "ADDX.w" | "ADDX.l" => {
            op & 0xf130 == 0xd100 && size < 3 && expected_size == Some(size)
        }
        "SUBX.b" | "SUBX.w" | "SUBX.l" => {
            op & 0xf130 == 0x9100 && size < 3 && expected_size == Some(size)
        }
        "ASL.b" | "ASL.w" | "ASL.l" | "ASR.b" | "ASR.w" | "ASR.l" | "LSL.b" | "LSL.w" | "LSL.l"
        | "LSR.b" | "LSR.w" | "LSR.l" | "ROL.b" | "ROL.w" | "ROL.l" | "ROR.b" | "ROR.w"
        | "ROR.l" | "ROXL.b" | "ROXL.w" | "ROXL.l" | "ROXR.b" | "ROXR.w" | "ROXR.l" => {
            shift_cohort_matches(op, &case.cohort, expected_size)
        }
        "BTST" | "BCHG" | "BCLR" | "BSET" => {
            let operation = match case.cohort.as_str() {
                "BTST" => 0,
                "BCHG" => 1,
                "BCLR" => 2,
                _ => 3,
            };
            (op & 0xff00 == 0x0800 || op & 0xf100 == 0x0100 && (op >> 3) & 7 != 1)
                && size == operation
        }
        "MULU" => op & 0xf1c0 == 0xc0c0,
        "MULS" => op & 0xf1c0 == 0xc1c0,
        "DIVU" => op & 0xf1c0 == 0x80c0,
        "DIVS" => op & 0xf1c0 == 0x81c0,
        "CHK" => op & 0xf1c0 == 0x4180,
        _ => false,
    }
}

pub(super) fn route_unsupported(case: &Case) -> Option<&'static str> {
    let op = case.initial.prefetch[0];
    let mode = (op >> 3) & 7;
    let size = (op >> 6) & 3;
    if matches!(
        case.cohort.as_str(),
        "MOVE.b" | "MOVE.w" | "MOVE.l" | "MOVEA.w" | "MOVEA.l"
    ) {
        let destination = (op >> 6) & 7;
        return (mode == 7 && op & 7 > 4
            || destination == 7 && (op >> 9) & 7 > 1
            || op >> 12 == 1 && (mode == 1 || destination == 1))
            .then_some("move_invalid_operand_form");
    }
    if matches!(
        case.cohort.as_str(),
        "TST.b"
            | "TST.w"
            | "TST.l"
            | "CLR.b"
            | "CLR.w"
            | "CLR.l"
            | "NEGX.b"
            | "NEGX.w"
            | "NEGX.l"
            | "NEG.b"
            | "NEG.w"
            | "NEG.l"
            | "NOT.b"
            | "NOT.w"
            | "NOT.l"
    ) {
        return (mode == 1 || mode == 7 && op & 7 > 1).then_some("unary_nonalterable_operand_form");
    }
    if matches!(case.cohort.as_str(), "JMP" | "JSR" | "LEA" | "PEA") {
        return (!(matches!(mode, 2 | 5 | 6) || mode == 7 && op & 7 <= 3))
            .then_some("control_invalid_address_form");
    }
    if case.cohort == "Scc" {
        return (mode == 1 || mode == 7 && op & 7 > 1)
            .then_some("condition_invalid_destination_form");
    }
    if case.cohort == "MOVEfromSR" {
        return (mode == 1 || mode == 7 && op & 7 > 1).then_some("status_invalid_destination_form");
    }
    if matches!(case.cohort.as_str(), "MOVEtoSR" | "MOVEtoCCR") {
        return (mode == 1 || mode == 7 && op & 7 > 4).then_some("status_invalid_source_form");
    }
    if case.cohort.starts_with("MOVEM.") {
        let r = op & 7;
        let legal = if op & 1024 != 0 {
            matches!(mode, 2 | 3 | 5 | 6) || mode == 7 && r <= 3
        } else {
            matches!(mode, 2 | 4 | 5 | 6) || mode == 7 && r <= 1
        };
        return (!legal).then_some("multiple_invalid_operand_form");
    }
    let admitted = match case.cohort.as_str() {
        "EXT.w" | "EXT.l" | "SWAP" => true,
        "RESET" | "RTE" | "RTS" | "ILLEGAL_LINEA" | "ILLEGAL_LINEF" => true,
        "MOVEP.w" | "MOVEP.l" => true,
        "BSR" | "DBcc" => true,
        "LINK" | "UNLINK" | "RTR" | "TRAP" | "MOVEfromUSP" | "MOVEtoUSP" => true,
        "ORItoCCR" | "ORItoSR" | "ANDItoCCR" | "ANDItoSR" | "EORItoCCR" | "EORItoSR" => true,
        cohort if binary_family(cohort).is_some() => return binary_unsupported(op),
        "EXG" => matches!(op & 0xf1f8, 0xc140 | 0xc148 | 0xc188),
        "ABCD" | "SBCD" | "ADDX.b" | "ADDX.w" | "ADDX.l" | "SUBX.b" | "SUBX.w" | "SUBX.l" => true,
        "NBCD" => {
            return (mode == 1 || mode == 7 && op & 7 > 1).then_some("bcd_invalid_operand_form");
        }
        "ASL.b" | "ASL.w" | "ASL.l" | "ASR.b" | "ASR.w" | "ASR.l" | "LSL.b" | "LSL.w" | "LSL.l"
        | "LSR.b" | "LSR.w" | "LSR.l" | "ROL.b" | "ROL.w" | "ROL.l" | "ROR.b" | "ROR.w"
        | "ROR.l" | "ROXL.b" | "ROXL.w" | "ROXL.l" | "ROXR.b" | "ROXR.w" | "ROXR.l" => {
            return (size == 3 && (mode < 2 || mode == 7 && op & 7 > 1))
                .then_some("shift_invalid_operand_form");
        }
        "BTST" | "BCHG" | "BCLR" | "BSET" => {
            let static_bit = op & 0xff00 == 0x0800;
            let maximum = if case.cohort == "BTST" {
                if static_bit { 3 } else { 4 }
            } else {
                1
            };
            return (mode == 1 || mode == 7 && op & 7 > maximum)
                .then_some("bit_invalid_operand_form");
        }
        "MULU" | "MULS" | "DIVU" | "DIVS" | "CHK" => {
            return (mode == 1 || mode == 7 && op & 7 > 4)
                .then_some("arithmetic_invalid_source_form");
        }
        _ => false,
    };
    if !admitted {
        return Some("nonregister_or_extension_form");
    }
    None
}
