import init, { BrowserPeer } from './wasm/zeff_netplay_connect.js';
await init();
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
let active;
const status = text => { document.querySelector('#status').textContent = text; };
window.__probe = { ready: true, room: '', result: null, failure: null };

export async function run({ url, token, room = '' }) {
    if (active) throw new Error('Already connected');
    const parsed = new URL(url);
    if ((parsed.protocol !== 'wss:' && !(parsed.protocol === 'ws:' && ['localhost', '127.0.0.1', '[::1]'].includes(parsed.hostname)))
        || parsed.username || parsed.password || parsed.search || parsed.hash) throw new Error('Use WSS, or loopback WS for testing');
    const ws = new WebSocket(url);
    let peer;
    active = { ws, close: () => peer?.close() };
    const messages = [];
    let ended = false;
    let deadline = Date.now() + 120000;
    const assert = (condition, reason) => { if (!condition) throw new Error(reason); };
    ws.onmessage = event => {
        if (typeof event.data !== 'string' || event.data.length > 40960 || messages.length >= 8) { ws.close(); ended = true; return; }
        try { messages.push(JSON.parse(event.data)); } catch { ws.close(); ended = true; }
    };
    ws.onclose = () => { ended = true; };
    ws.onerror = () => { ended = true; };
    const receive = async () => {
        while (!messages.length) {
            assert(!ended && Date.now() < deadline, 'Lobby disconnected or timed out');
            await sleep(5);
        }
        const message = messages.shift();
        assert(message.type !== 'error', `Lobby: ${message.code}`);
        return message;
    };
    try {
        while (ws.readyState === WebSocket.CONNECTING) {
            assert(Date.now() < deadline, 'Lobby connection timed out'); await sleep(10);
        }
        assert(ws.readyState === WebSocket.OPEN, 'Lobby unavailable');
        const host = !room;
        ws.send(JSON.stringify({ type: host ? 'create' : 'join', version: 1, access_token: token,
            ...(host ? {} : { room }), identity: { core: 'transport-proof', content_hash: '1'.repeat(64),
                compatibility_hash: '2'.repeat(64), mode: 'shared_console' } }));
        const welcome = await receive();
        assert(welcome.type === 'welcome' && welcome.version === 1 && /^[a-f0-9]{24}$/.test(welcome.room), 'Invalid welcome');
        assert(welcome.role === (host ? 'host' : 'guest'), 'Wrong peer role');
        window.__probe.room = welcome.room;
        document.querySelector('#room').value = welcome.room;
        status(`Room: ${welcome.room}\nWaiting for peer…`);
        peer = new BrowserPeer(JSON.stringify(welcome.ice_servers), welcome.relay_allowed);
        peer.create_channels();
        if (host) {
            assert((await receive()).type === 'peer_joined', 'Expected peer');
            ws.send(JSON.stringify({ type: 'signal', signal: { type: 'offer', sdp: await peer.offer() } }));
        }
        const remote = await receive();
        assert(remote.type === 'signal' && remote.signal.type === (host ? 'answer' : 'offer'), 'Expected description');
        await peer.apply_remote(remote.signal.sdp, !host);
        if (!host) ws.send(JSON.stringify({ type: 'signal', signal: { type: 'answer', sdp: await peer.answer() } }));
        while (!peer.ready()) {
            assert(!peer.failure() && Date.now() < deadline, peer.failure() || 'Peer connection timed out'); await sleep(5);
        }
        ws.send(JSON.stringify({ type: 'finish' }));
        assert((await receive()).type === 'complete', 'Expected completed negotiation');
        ws.close();
        const closeDeadline = Date.now() + 5000;
        while (ws.readyState !== WebSocket.CLOSED) {
            assert(Date.now() < closeDeadline, 'Lobby close timed out'); await sleep(5);
        }
        status('Connected directly. Testing packets…');
        const packet = (phase, sequence, kind) => {
            const bytes = new Uint8Array(64);
            bytes.set([90, 78, 80, 49, phase, kind]);
            new DataView(bytes.buffer).setUint32(6, sequence, true);
            for (let i = 10; i < bytes.length; i++) bytes[i] = (sequence * 13 + i + phase) & 255;
            return bytes;
        };
        const receivePacket = async () => {
            const end = Date.now() + 10000;
            for (;;) {
                const p = peer.take_packet(); if (p) return p;
                assert(!peer.failure() && Date.now() < end, peer.failure() || 'Packet timed out'); await sleep(1);
            }
        };
        const equal = (a,b) => a.length === b.length && a.every((v,i) => v === b[i]);
        const phases = [];
        for (const [phase, count] of [[0,240],[1,8]]) {
            let hash = 0xcbf29ce484222325n;
            for (let sequence = 0; sequence < count; sequence++) {
                const expected = [packet(phase, sequence, 0), packet(phase, sequence, 1)];
                if (host) for (let kind = 0; kind < 2; kind++) peer.send(kind, expected[kind]);
                const seen = new Set();
                for (let i = 0; i < 2; i++) {
                    const p = await receivePacket(); const kind = p[0]; const bytes = p.slice(1);
                    assert(kind < 2 && !seen.has(kind) && equal(bytes, expected[kind]), 'Packet bytes differ');
                    seen.add(kind); if (!host) peer.send(kind, bytes);
                }
                for (const bytes of expected) for (const b of bytes) hash = BigInt.asUintN(64, (hash ^ BigInt(b)) * 0x100000001b3n);
            }
            phases.push({ phase, count, hash: hash.toString(16).padStart(16,'0') });
        }
        const encoder = new TextEncoder(); const decoder = new TextDecoder();
        const expectControl = async text => { const p = await receivePacket(); assert(p[0] === 0 && decoder.decode(p.slice(1)) === text, 'Close barrier differs'); };
        if (host) {
            peer.send(0, encoder.encode('proof-close-ready')); await expectControl('proof-close-ack');
            peer.send(0, encoder.encode('proof-close-done')); await sleep(100);
        } else {
            await expectControl('proof-close-ready'); peer.send(0, encoder.encode('proof-close-ack')); await expectControl('proof-close-done');
        }
        const result = { after_signaling_close: true, phases, packets_per_channel: 248, relay_allowed: welcome.relay_allowed };
        window.__probe.result = result;
        status('Passed: 248 packets in each direction on both channels.\nLobby disconnected before packet exchange.');
        return result;
    } catch (e) {
        window.__probe.failure = String(e); status(String(e)); throw e;
    } finally {
        ws.close(); peer?.close(); peer?.free(); active = null;
    }
}

window.__probe.run = run;
document.querySelector('#connect').onclick = () => run({ url: document.querySelector('#url').value,
    token: document.querySelector('#token').value, room: document.querySelector('#room').value }).catch(() => {});
document.querySelector('#stop').onclick = () => { active?.close(); active?.ws.close(); };
