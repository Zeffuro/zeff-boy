#!/usr/bin/env python3
"""Fetch pinned, licensed M68000 architectural witnesses without sampling."""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import struct
import urllib.request

COMMIT = "64b253116a3de04aaac4346c43680960dc9b67e5"
BASE = f"https://raw.githubusercontent.com/SingleStepTests/m68000/{COMMIT}/"
HASHES = {
    "NOP": "9cebf91f85b38304e2496c87613621cbb983d0274a083295492ca25e2331c2b2",
    "MOVE.q": "bfbe824891e79b72b7163cf40f2c338a40ce50510926cee2981ecc55fb687c13",
    "MOVE.b": "b4400d361e6ef1f6dada70c613e95039161ad8142dfcb64b5cfa27bd5ee24959",
    "MOVE.w": "11c25e839789cb3902d12c0901b707fc5d039008424013375a03d6216efdf653",
    "MOVE.l": "5b3c5b8de96ca76bfc2b12033541f31048d12a17172f6f286f59314450e8cc15",
    "MOVEA.w": "2de2210c3d129584c292faf6d2124af1d59c2ab715ff9025f642bdf5e230c23e",
    "MOVEA.l": "3329a76915e9b78dbf8ef0db4260339ab0458c9b4bfb22aabddfcf0f534c344c",
    "Bcc": "1f3df93507e9be800d8cced3d7f9f59d2b562e3f23b46badc23b5bb459383ed9",
    "BSR": "5e071f3914e0495a22e7e7d61848c3193e9c3b8f27a70bfa35d083f1e66b458f",
    "LINK": "cb1ab4e4f812c11eef19b980e52401c0ce2ea82551be26fadb9406daa2e6d550",
    "UNLINK": "ed9327770747232b80cec2c1b0c8fabdfef2f1067af78a6b21f7c20b407d78e4",
    "RTS": "772f0bde0b6c82227edbcd374364f6ae07da6c5fa01ca469a368a053d57c5b63",
    "RTE": "6f2de0d6abc51528ff62498af4e61b311102300f9f4d5fb825681116b3a0249c",
}
BASE_COHORTS = tuple(HASHES)
HASHES.update({
    "ADD.b": "3d76bf7b85fc237be7ef2015445e91490677443d2bd4d6e6d84886989322cae5",
    "ADD.l": "cc19a1d614a9ce4a68e3e85a108a3d88811348a748b38950372c09bd947e711e",
    "ADD.w": "bd596a3d2ed49b0e9619643dd1d85b07b05993465fde440ab011221561e629a2",
    "ADDA.l": "a68dc7807e3705d56c9fd0615316c9fd17aed120fce74ef26fe4b796dfd0b3b2",
    "ADDA.w": "057ddac58789c8da190494a472cb6b591411d307b4d2363b31bb6ec195ee98e7",
    "AND.b": "dcc72a8ecf74ca6efc61a743ffdc475c448d8c0b517ae44b72044f6fc62246e4",
    "AND.l": "bde6f203725d64dcd0e09244f5c127a0323ab16037626075cc965a3782fbcb38",
    "AND.w": "72f0e3ff75f1fff5f2ad221d5dcae703d7aff3256e77338a1a1594174a415e43",
    "ANDItoCCR": "22e73b8f5c22b375d23f901856f13a9fbe463e4ae1f8a88a12dc640bc8bd878d",
    "ANDItoSR": "6636de1c0521d818ab34683b5278e60f6b89607557da631e887960ad8b3cc487",
    "CLR.b": "7f6d569ed57b5bf3e57e422802bb256884f2290e641ca5e6279376e5ee23170c",
    "CLR.l": "f41e6a53171fb720d14ca8897e4ccc5c70aff6f78c6f241da9d4ddcba6058d0f",
    "CLR.w": "e7ac8f9efb9c71f88bc3930844e5b2f27eb61635761ab9a36ada767185acdd7a",
    "CMP.b": "577ea026c89e6f8c722d343cc42eb524c43d6861d205ce45abe65d9bfbb86ce5",
    "CMP.l": "fa6c930fc6ac273308a29d1a3e535dc3be4632071299a572b7da934e5cb743d8",
    "CMP.w": "628adb87b4aa54c8f050ceb5be543139204461eb56502c15abaaa68560785d38",
    "CMPA.l": "efd3c67626c60e4ec07e6174b8f98a8b302ba712310a041e3a516154524a40b2",
    "CMPA.w": "f49365f9816aa621f2a00b3e1b9eb96038221b1504a4c20bf5b5a4ec61cdee2f",
    "DBcc": "69464a7cfc6bfdff0edb7ef4a0c8e49baa15d9809913c3261de29e76c263ca0a",
    "EOR.b": "87bef8de9bb884665e411dd59a501aa2c10348cbe71127c332a139228df20250",
    "EOR.l": "5de170b5c84f7a85f05936651fdb7b8653b5aa5b26360aaad525f94aed9a0373",
    "EOR.w": "065fdad8329184f0154e04a1b678df32eb62a260f46ae487c2f854dcd28cce9f",
    "EORItoCCR": "9874e4db5b0dc5b6f68b8d8de947a1a80b50ca5d9fe0fd1faf5946112d2ca91f",
    "EORItoSR": "e0eb3039e3d117e5c52e94133cb0a7594a3a7251e378db5e5a8385822afa7117",
    "EXG": "6d1990dc8615dd2e0cb7193255e537827902f863ba09414c5e6091287b41de87",
    "EXT.l": "8aa149103720d7cdfd17ce1646ab381e0bf0baa41039c20e153851ff8d46b4af",
    "EXT.w": "d7210993a82ca83ba10d34171558f6f3fa2539f02b7a6a3886b1f61d716c35e9",
    "JMP": "2d64918cd078fcb174a5bdb60a24141cecc982b89304dc8b3af68b4055feb5b4",
    "JSR": "41943a8c373c29736aa70986553f31bb03d6be5f4a70aed1ce383ec05bdd0f67",
    "LEA": "4f30ade96e6cf65c0628b940858899f1e0db2b40b014207509c9881c4b72be29",
    "MOVEfromSR": "c6a416d1e93c31d1901a1296f08e7cb14e99cf8dbe5c6541878b1956751fb725",
    "MOVEfromUSP": "7dcd639e8f8619b78581940397ba5f07ad4378d879ae3b4fa90e5e2979379947",
    "MOVEtoCCR": "603ffea2c9109daa2358288934337a52976636c59d008a75a5ba71d70b304729",
    "MOVEtoSR": "cf7ce17aacfc6be5a9db9456bd7d7d70206c13e456691245011064e6c0a43d75",
    "MOVEtoUSP": "f43a627e07e59c5bfc83f11ef756b1c743d0249186beb7b3f12ea16b66ea7a19",
    "NEG.b": "8f4e24d794d6f6d3c824398848b95e6fdfab0af7d2bcf1399d1c68a58b9bf683",
    "NEG.l": "52bbcf950c31a102340283135284798680f936789955e7918c0b55e69898e215",
    "NEG.w": "43f173304b82e4931cabc5b37744f12b617dee9d8bced8c071876a2c4e4c413e",
    "NOT.b": "973f147aa25b58c3fc708edb240b884836ffdc472d5bc923f14b38d56d8680b9",
    "NOT.l": "1809fb0b9dc2f657c453b5881da11bb5d813dcd2d909159bc4a8d3320591c3fc",
    "NOT.w": "5ff8f1b45297c0aef0078075976ff1751c18557b1049f1c1f4d6eef2bf66445b",
    "OR.b": "62add1222e705a287e23b4385ea45cc4ea9bd26f9b7c845d0cab06effaa9a1e3",
    "OR.l": "649a3e0bc99758e01b237d28b7452f002fde28d770200034382f9d944a8182f4",
    "OR.w": "4d741c955f7edae2a2f5f12129f05fc28342a0ad5798552b2f177affa2cfd0d8",
    "ORItoCCR": "34a23aee7ddcfdf113e988ab71d8f093512193d421b17f3abfdf356ff3663c92",
    "ORItoSR": "783008bbc9e27ac0be3e5928ce7353dce4eb2a44e52a2d7eed660921f44796a6",
    "PEA": "13f7db5a0c05df2b381778ccdfd228f3751d0f36a104803e5fc4d5d0545c0d88",
    "SUB.b": "3201511e0398f360a6b27fc8ef3b04e747d5298aacdfdced8d6aba6dc39782f9",
    "SUB.l": "564976803df46dca0db22fcbdcc8ddfbd87d10ad6dcf62ed63f7c963b13da0e8",
    "SUB.w": "544902833ff09af419295b4a2bba70b5e55c2380b5444fe0107b89aa03fcb181",
    "SUBA.l": "788a2d5fac9563260f7771456425f484e812887a45a77b82d33cea1e70df4022",
    "SUBA.w": "cbc43f9db2e44dd8d869345047b444773f73ed0af376723987b7faabca30d0cb",
    "SWAP": "c68b48eca1bbfc9c43d9fd671a5b4bff2ca85b6c4a30415271da54317608205e",
    "Scc": "5937ecdc30935cae65c9d6e2edce144eb7ea082660533d99f13df848e3648686",
    "TRAP": "fead4435111103b49ccdef17f60852c438d81f21f1fd9a5ffcac0c2548ad7a85",
    "TST.b": "289681ae49b0a58b412663488c288bade03748638399aeeb4ed8fdd64d17dc2d",
    "TST.w": "0d16c2cfd8112aa82fbdda4883635ba31dba5810e14288762e0744429c2c7a78",
    "TST.l": "a37cfd0f43451d39491bb8425c2fe46270e5d43cf46e5029a1fd4cd66df65541"
})
M2A_COHORTS = tuple(HASHES)
HASHES.update({
    "ABCD": "8d84455744247f93c3127efdd13ec706296dc9e85b8bd66ef04b60a2c5a14a1d",
    "SBCD": "09954ec662dfded16bdd367792ce120b83659213a3a41ebd0b9caafdcb36fa46",
    "NBCD": "de37b0a1706048ff0bfb5894996c60d4be2a83aadfd59a12160a3a08982139d4",
    "MOVEM.l": "b89ca3a4acbdd0e303a41d9fccfb67184dc7706d50cbab48c116e19a5f592bdd",
    "MOVEM.w": "0856263b984602dd2ca55fa8eac7f99fd8b48862facb917813692665d68201f9",
    "MOVEP.l": "3af21c3ebb5fb4c142df3c2dbce25a5fb651c4a804d3691ca3f3a06c6365b16a",
    "MOVEP.w": "56e94fc662c68e7657c61d09849c8b20c986987be133b3f871aeab8956623d4c",
    "ADDX.b": "c58e24dcb3e5ea07788d9ee98bde51305c3fd3ebdf18979b7dc95ed279fe8cea",
    "ADDX.w": "f3d1f3a1f255452fcb0b83479510b006bc777d0a7fd1092a12942b3ff4af1785",
    "ADDX.l": "9fcaee5fe1c5f3c601ba68e21f77f7f73cda56c304a990991448854e290aab2e",
    "SUBX.b": "d9a014be40dbf81806cecd5368dd36d3f1754ec722a26530bb4bf230b86d2cc6",
    "SUBX.w": "1dcd356421bb89060f77f39168c13e9ca3449154b02988b1e1f252bb16526c61",
    "SUBX.l": "a54c7d57e876476db168e6da544da6455788158cba5f5468227ec78bca0872c9",
    "NEGX.b": "d01e2e4f9b9e453fbd43f3d4cda6b818ad1e26554cd21cc096e25f8c3b1f49c3",
    "NEGX.w": "8a85da17cd533d9444d78bb891620569b864da8c969f3a9330373e8d55a8c4d6",
    "NEGX.l": "ca59b55f388fe4d6849c4379fafb068ce2ef3d2fc6702898ed5aa63413c48d58",
    "MULU": "9995eb90ff8c4293c7f6440d667ef47a2e6f34d01f2389b07d38d58740ceb7e1",
    "MULS": "cbe138399d53590de3bdfd446d5cd13f11cd7a3753712405df2b130a62b18b2c",
    "DIVU": "91358e1e9296a0d130c08416fef2db866de6fdc6b1876e342d90ac3ef0038c6f",
    "DIVS": "7ed3590131098159d115b86b7be284750f7e410cea8d7e37a58f0f3f0a6b24ad",
    "CHK": "7daa978c27f6463aeb520195a8ae19cfdd2cfe76fdd860493fa1a6cd9879b440",
    "BTST": "085509a4341a447a3b9c83ffd73bda15402ddb409e30e875e507a290ac32b074",
    "BCLR": "2a5ce0b7e656a74c4bf4eb6cf0bf52cf00a3717b34d853cd563545047853902f",
    "BCHG": "a5508b0dda62f57553e7d4d7a4cb6fd2cf1f16c27d8beb883a6978853ed3a285",
    "BSET": "90249455984d1b9fe03f85f4739a7535142653084b472fa8c17246aebdf57bab",
    "ASL.b": "5bfda5992400671a0e54be142b625b2c26a225d11c35cc559e9eaa5c66203554",
    "ASL.w": "1f1c952d50a3419932e22633a9247b08212787be31517f6aaa294db42432b809",
    "ASL.l": "e44efd7fd77315877bebab514f5ede5941966a37eb1ccb0e8816a6f00d113815",
    "ASR.b": "bea8aa1c533071582d1adece5733457371883898ce0bff7045e224bf5320979d",
    "ASR.w": "3ad48e929e93d104c7df84d67e601d7f246826cc2466d2dadfdaecddcb072bc0",
    "ASR.l": "81c9ae3dd7be601e2d6b4ddd58b97ab42bd06cc4cdc0cadef35e7fe46fe0ac01",
    "LSL.b": "f294bf09b3b08149b7a53f8e7209eef979a035c14341ffbd84d885894cce0e17",
    "LSL.w": "c54f1c9edf495f4c47484c69562f68823cec8ce6903d9c408ecd79663e34e5ec",
    "LSL.l": "559f2d0bc1d729d2342a5a8ce3e8fe9a58eb47e29a8c4305ed861b7e5e7f6abb",
    "LSR.b": "3c66880461f46bcbc3edd02e89fdfe883ac324f3e9e3d872b4a5b3420ca3c241",
    "LSR.w": "9dd60d26081d4c8fc0b066f8a6a89b129aec0da4436ba2a514e3f98d79389b1c",
    "LSR.l": "2b8f9bbed2b781fa7282bb93cb834ad4f9ee737dd24bbdea9265f5460142a1a9",
    "ROL.b": "58b6b13f627a6a23d6a19188bc8cb00934fb97bab5d00cee499d2d159984ea7b",
    "ROL.w": "faa8775164afa38f336a8c73aaa651b4d00074bb5dd3b7f23b8c1cefaca9e01c",
    "ROL.l": "fe5f2199fc91ec4883bdb8de7c75dc9414a5f231d079aabe17d85ab81e74e99d",
    "ROR.b": "aabe4dfd9ff678450b75687cfb158396e4ea400eedeec527ce5af154c2e46bee",
    "ROR.w": "0e21a5bd6710b7292344fc8a628461f47909117e2d02d9143868ec5b27cb96ad",
    "ROR.l": "425585edcbbd9ff8af771f2cb64e4fa88da6f2060284c30e0a7b8062efd4b2ed",
    "ROXL.b": "b55b34221f11dcd0c840acd28ba2f7a6a45bd0c7f551b082f73457987f117c04",
    "ROXL.w": "f7abf0f729c8d4e6554d2a96d1f40156802e5c5f639bbfd2f5a8c03c070ec6ef",
    "ROXL.l": "48fa8ce9843d6ec36a3f50751fe13515bca04d0f38feddf4155639409cd3d258",
    "ROXR.b": "a144ad40461f7e9c5985a22409345884f6ce7a319fd3c6b4de9ef7188b34d747",
    "ROXR.w": "a21dc7cd3cb190aae08da002e4b88daa895d23cc9fd4687ad2fe40035d741629",
    "ROXR.l": "c8ea216503d02189e4cdbe66fefbd8cb7713a3815c30799d8e750f20ee802385"
})
HASHES.update({
    "RTR": "05f83705c724ca94f9af80b9afab3800fee92c94c9a398a8fd774c9604727f6c",
    "ILLEGAL_LINEA": "83cff2cf6c8f39b849d17f1e4bcf4c9ad5de1349d91599b25ff19a2c08619353",
    "ILLEGAL_LINEF": "315e707ae69aedd927611d8467755b7ac5c9edb1623bad477f13e3bbe4d7f153"
})
M2B_COHORTS = tuple(HASHES)
HASHES["RESET"] = "41bd1d0431480185d2061e171298810f7f5165c3cb819f3f502ccc212bf6f768"
REGISTERS = [f"d{i}" for i in range(8)] + [f"a{i}" for i in range(7)]
REGISTERS += ["usp", "ssp", "sr", "pc"]
MASK = 0xFFFFFF
LICENSE_HASH = "9b9e332ee11019f131e945d40e20ab0c48b52921754dc03fe62cc85b579e6c61"
README_HASH = "7f36eed9cb93f061d13bfd18dc310c9bda7b5f3ac96b6d0dfe292e00612c46b6"


def digest(data):
    return hashlib.sha256(data).hexdigest()


def fetch(cache, name, expected=None):
    path = cache / name
    if not path.exists():
        with urllib.request.urlopen(BASE + name, timeout=60) as response:
            data = response.read(32 * 1024 * 1024 + 1)
        if len(data) > 32 * 1024 * 1024:
            raise ValueError(f"oversized source: {name}")
        if expected and digest(data) != expected:
            raise ValueError(f"source hash mismatch: {name}")
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
    data = path.read_bytes()
    if expected and digest(data) != expected:
        raise ValueError(f"cached source hash mismatch: {name}")
    return data


class Decoder:
    def __init__(self, data):
        self.data = data
        self.offset = 0

    def read(self, fmt):
        result = struct.unpack_from("<" + fmt, self.data, self.offset)
        self.offset += struct.calcsize("<" + fmt)
        return result

    def header(self, expected):
        start = self.offset
        length, magic = self.read("II")
        if magic != expected:
            raise ValueError(f"binary schema magic mismatch at {self.offset - 8}")
        if length < 8 or start + length > len(self.data):
            raise ValueError("invalid binary section length")
        return start + length

    def end(self, expected):
        if self.offset != expected:
            raise ValueError("binary section length mismatch")

    def state(self):
        end = self.header(0x01234567)
        state = dict(zip(REGISTERS, self.read("I" * len(REGISTERS))))
        state["prefetch"] = self.read("II")
        ram = {}
        for _ in range(self.read("I")[0]):
            address, word = self.read("IH")
            if address > MASK or address & 1:
                raise ValueError("invalid word RAM address")
            ram[address], ram[address + 1] = word >> 8, word & 255
        state["ram"] = ram
        self.end(end)
        return state

    def tests(self):
        magic, count = self.read("II")
        if magic != 0x1A3F5D71:
            raise ValueError("unknown corpus format")
        for _ in range(count):
            test_end = self.header(0xABC12367)
            name_end = self.header(0x89ABCDEF)
            length = self.read("I")[0]
            name = self.data[self.offset:self.offset + length].decode("utf-8")
            self.offset += length
            self.end(name_end)
            initial, final = self.state(), self.state()
            transfers_end = self.header(0x456789AB)
            self.total_cycles, transfers = self.read("II")
            transactions = []
            self.phase_transactions = []
            for _ in range(transfers):
                kind, duration = self.read("BI")
                if kind not in range(6):
                    raise ValueError("unknown transaction kind")
                phase = {"kind": ("idle", "write", "read", "tas", "read_address_error", "write_address_error")[kind],
                         "duration_cycles": duration}
                if kind:
                    fields = self.read("IIIII")
                    transactions.append((kind, *fields))
                    phase.update(zip(("function_code", "address_bus", "data_bus", "uds", "lds"), fields))
                self.phase_transactions.append(phase)
            self.end(transfers_end)
            self.end(test_end)
            yield name, initial, final, transactions
        if self.offset != len(self.data):
            raise ValueError("unconsumed corpus bytes")


def state_json(state):
    return {"d": [state[f"d{i}"] for i in range(8)],
            "a": [state[f"a{i}"] for i in range(7)],
            "usp": state["usp"], "ssp": state["ssp"], "sr": state["sr"],
            "pc": (state["pc"] - 4) & 0xFFFFFFFF}


def instruction_family(opcode, cohort):
    if cohort == "MOVE.q":
        return "MOVEQ"
    if opcode & 0xF138 == 0xB108 and opcode & 0xC0 != 0xC0:
        return "CMPM"
    if opcode >> 12 == 0 and ((opcode >> 8) & 15) in (0, 2, 4, 6, 10, 12):
        return {0: "ORI", 2: "ANDI", 4: "SUBI", 6: "ADDI", 10: "EORI", 12: "CMPI"}[(opcode >> 8) & 15]
    if opcode >> 12 == 5 and opcode & 0xC0 != 0xC0:
        return "SUBQ" if opcode & 0x100 else "ADDQ"
    return cohort.split(".")[0]


def normalize(name, initial, final, transactions, *, bus_contracts=False):
    if initial["sr"] & 0x8000:
        return None, "trace"
    if any(t[0] in (4, 5) for t in transactions):
        return None, "address_fault"
    opcode = initial["prefetch"][0]
    vector = None
    if opcode & 0xFFF0 == 0x4E40:
        vector = 32 + (opcode & 15)
    elif opcode & 0xF1C0 == 0x4180:
        vector = 6
    elif opcode & 0xF1C0 in (0x80C0, 0x81C0):
        vector = 5
    elif opcode >> 12 in (10, 15):
        vector = 10 if opcode >> 12 == 10 else 11
    elif bus_contracts and opcode == 0x4E70:
        vector = 8
    vector_reads = {vector * 4, vector * 4 + 2} if vector is not None else set()
    if any(t[0] == 2 and t[1] == 5 and t[2] < 0x400 and t[2] not in vector_reads for t in transactions):
        return None, "exception_or_low_supervisor_data"
    memory = dict(initial["ram"])
    code = set()
    for state in (initial, final):
        for offset in range(4):
            code.add((state["pc"] - 4 + offset) & MASK)
    for i, word in enumerate(initial["prefetch"]):
        address = (initial["pc"] - 4 + 2 * i) & MASK
        for position, value in ((address, word >> 8), ((address + 1) & MASK, word & 255)):
            if position in memory and memory[position] != value:
                return None, "prefetch_ram_disagreement"
            memory[position] = value
    for kind, fc, address, _, upper, lower in transactions:
        addresses = ({address & MASK} if upper else set()) | ({(address + 1) & MASK} if lower else set())
        if fc in (2, 6):
            code.update(addresses)
    for kind, fc, address, _, upper, lower in transactions:
        addresses = ({address & MASK} if upper else set()) | ({(address + 1) & MASK} if lower else set())
        if fc in (1, 5) and addresses & code:
            return None, "data_prefetch_overlap"
    if any(t[0] == 1 and ({t[2] & MASK, (t[2] + 1) & MASK} & code) for t in transactions):
        return None, "self_modifying_code"
    selected = set()
    written = set()
    for kind, _, address, _, upper, lower in transactions:
        if kind in (1, 2):
            if upper:
                selected.add(address & MASK)
                if kind == 1:
                    written.add(address & MASK)
            if lower:
                selected.add((address + 1) & MASK)
                if kind == 1:
                    written.add((address + 1) & MASK)
    expected = dict(memory)
    padding = 0
    for address, value in final["ram"].items():
        if address not in memory and address not in selected:
            # Sparse final word snapshots include an unwitnessed zero beside a new byte write.
            if value != 0 or (address ^ 1) not in written:
                raise ValueError("unknown final-only memory convention")
            padding += 1
        else:
            expected[address] = value
    return {"name": name, "initial": state_json(initial), "final": state_json(final),
            "memory": sorted(memory.items()), "expected_memory": sorted(expected.items()),
            "_unobserved_word_padding_bytes": padding}, None


def phase_state(state):
    result = {register: state[register] for register in REGISTERS}
    result["prefetch"] = list(state["prefetch"])
    result["ram"] = sorted(state["ram"].items())
    return result


def export_phases(cohorts, cache, output, *, bus_contracts=False):
    output.parent.mkdir(parents=True, exist_ok=True)
    ledger = []
    export_hash = hashlib.sha256()
    claim = "lossless reference phase intake; no emulator phase, transfer timing, prefetch or physical hardware validation"
    with output.open("w", encoding="utf-8", newline="\n") as stream:
        def emit(row):
            encoded = json.dumps(row, separators=(",", ":")) + "\n"
            stream.write(encoded)
            export_hash.update(encoded.encode("utf-8"))

        emit({"type": "header", "schema": 1, "format": "m68000-reference-phases-jsonl",
              "source_commit": COMMIT, "cohorts": cohorts, "claim": claim,
              "ram_convention": "16-bit source words retained as big-endian byte pairs",
              "pc_convention": "raw next-prefetch PC retained; no PC normalization",
              "clock_convention": "record total and ordered transaction durations retained separately; no synthesized timeline"})
        for cohort in cohorts:
            raw = fetch(cache, f"v1/{cohort}.json.bin", HASHES[cohort])
            decoder = Decoder(raw)
            excluded, total, eligible = Counter(), 0, 0
            for name, initial, final, transactions in decoder.tests():
                _, reason = normalize(name, initial, final, transactions, bus_contracts=bus_contracts)
                total += 1
                if reason:
                    excluded[reason] += 1
                else:
                    eligible += 1
                emit({"type": "case", "cohort": cohort, "name": name,
                      "initial": phase_state(initial), "final": phase_state(final),
                      "reference_total_cycles": decoder.total_cycles,
                      "transactions": decoder.phase_transactions,
                      "normalization_exclusion": reason})
            ledger.append({"cohort": cohort, "raw_sha256": digest(raw), "total": total,
                           "eligible": eligible, "excluded": dict(sorted(excluded.items())),
                           "exported": total})
        emit({"type": "complete", "exported_records": sum(row["total"] for row in ledger)})
    receipt = {"schema": 1, "format": "m68000-reference-phases-jsonl", "source": BASE,
               "source_commit": COMMIT, "license": "MIT", "license_sha256": LICENSE_HASH,
               "readme_sha256": README_HASH, "export_sha256": export_hash.hexdigest(),
               "exported_records": sum(row["total"] for row in ledger), "cohorts": ledger,
               "claim": claim}
    output.with_suffix(".receipt.json").write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cohorts", nargs="+", choices=list(HASHES))
    selection = parser.add_mutually_exclusive_group()
    selection.add_argument("--expanded", action="store_true", help="admit the 71-cohort M2A checkpoint")
    selection.add_argument("--m2b", action="store_true", help="admit all 123 M2B cohorts into a separate fixture")
    selection.add_argument("--bus-contracts", action="store_true", help="admit 124 cohorts including RESET")
    parser.add_argument("--nominal-cycles", action="store_true", help="also compare the reference instruction clock total")
    parser.add_argument("--phase-export", action="store_true", help="stream all raw records as JSONL; requires explicit --cohorts")
    parser.add_argument("--cache", type=Path)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if args.phase_export and not args.cohorts:
        parser.error("--phase-export requires explicit --cohorts")
    root = Path(__file__).resolve().parents[1]
    cache = args.cache or root / "rom-tests/cache/sources/m68000-64b25311"
    prefix = "nominal" if args.nominal_cycles else "architectural"
    suffix = "-bus" if args.bus_contracts else "-m2b" if args.m2b else "-expanded" if args.expanded else ""
    filename = "reference-phases-v1.jsonl" if args.phase_export else f"{prefix}{suffix}-v1.json"
    output = args.output or root / "rom-tests/cache/m68k" / filename
    license_data = fetch(cache, "LICENSE", LICENSE_HASH)
    readme = fetch(cache, "README.md", README_HASH)
    if b"MIT License" not in license_data or b"+4 from where the test starts" not in readme:
        raise ValueError("license or PC convention unknown; refusing normalization")
    if args.phase_export:
        receipt = export_phases(list(dict.fromkeys(args.cohorts)), cache, output,
                                bus_contracts=args.bus_contracts)
        print(json.dumps(receipt, indent=2))
        return
    fixtures, ledger = [], []
    cohorts = args.cohorts or (list(HASHES) if args.bus_contracts else list(M2B_COHORTS) if args.m2b
                               else list(M2A_COHORTS) if args.expanded else list(BASE_COHORTS))
    for cohort in dict.fromkeys(cohorts):
        filename = f"v1/{cohort}.json.bin"
        raw = fetch(cache, filename, HASHES[cohort])
        excluded, total, eligible, padding = Counter(), 0, 0, 0
        families, eligible_families = Counter(), Counter()
        decoder = Decoder(raw)
        for name, initial, final, transactions in decoder.tests():
            total += 1
            family = instruction_family(initial["prefetch"][0], cohort)
            families[family] += 1
            fixture, reason = normalize(name, initial, final, transactions, bus_contracts=args.bus_contracts)
            if reason:
                excluded[reason] += 1
            else:
                eligible_families[family] += 1
                padding += fixture.pop("_unobserved_word_padding_bytes")
                fixture["cohort"] = cohort
                if args.nominal_cycles:
                    fixture["expected_nominal_cycles"] = decoder.total_cycles
                fixtures.append(fixture)
                eligible += 1
        ledger.append({"cohort": cohort, "raw_sha256": digest(raw), "total": total,
                       "eligible": eligible, "excluded": dict(sorted(excluded.items())),
                       "unobserved_word_padding_bytes": padding,
                       "opcode_families": {family: {"total": count, "eligible": eligible_families[family]}
                                           for family, count in sorted(families.items())}})
    output.parent.mkdir(parents=True, exist_ok=True)
    payload = {"schema": 1, "source_commit": COMMIT, "cases": fixtures}
    output.write_text(json.dumps(payload, separators=(",", ":")), encoding="utf-8")
    receipt = {"source": BASE, "commit": COMMIT, "license": "MIT",
               "license_sha256": digest(license_data), "readme_sha256": digest(readme),
               "pc_convention": "next-prefetch minus four", "normalized_sha256": digest(output.read_bytes()),
               "total_eligible": len(fixtures), "cohorts": ledger,
               "nominal_cycle_comparison": args.nominal_cycles,
               "memory_convention": "compare initial bytes and all final transaction-selected bytes; omit only final-only zero padding in an unselected word lane",
               "coverage_limits": {"STOP": "not selected: unchanged next-prefetch PC and no explicit stopped-state witness; final architectural PC convention unknown",
                                   "TRAPV": "not selected: upstream README marks oracle unverified",
                                   "TAS": "not selected: upstream README warns its special read-modify-write timing is not handled",
                                   "RESET": ("selected: state and optional instruction totals only; external reset pulse output/order not witnessed by reference"
                                             if "RESET" in cohorts else "not selected in this checkpoint"),
                                   "ILLEGAL": "no ordinary ILLEGAL opcode file; LINEA/LINEF use separate short-exception cohorts",
                                   "trace_interrupt_group0_prefetch_transfer_timing": "outside admission"},
               "claim": ("architectural state, memory and reference nominal instruction totals; no per-transfer timing, prefetch or physical hardware proof"
                         if args.nominal_cycles else "architectural state and memory only; no cycles, prefetch or physical hardware proof")}
    receipt_path = output.with_suffix(".receipt.json")
    receipt_path.write_text(json.dumps(receipt, indent=2) + "\n", encoding="utf-8")
    print(json.dumps(receipt, indent=2))


if __name__ == "__main__":
    main()
