import hashlib
import json
from pathlib import Path

from netplay_production_evidence import require


def selected(path):
    if path is None:
        return []
    entries = json.loads(path.read_text(encoding='utf-8'))
    require(isinstance(entries, list) and 0 < len(entries) <= 13, 'media coverage exceeds pair limit')
    games = []
    for entry in entries:
        source = (path.parent / entry['path']).resolve()
        require(source.stat().st_size <= 1024 * 1024, 'qualification media exceeds limit')
        data = source.read_bytes()
        require(len(data) >= 16 and data[:4] == b'NES\x1a', 'direct NES media required')
        digest = hashlib.sha256(data).hexdigest()
        require(digest == entry['sha256'], 'selected media bytes changed')
        nes2 = data[7] & 12 == 8
        mapper = (data[6] >> 4) | (data[7] & 0xf0)
        mapper |= (data[8] & 15) << 8 if nes2 else 0
        timing = data[12] & 3 if nes2 else data[9] & 1
        require(timing in (0, 1, 3), 'resolved qualification timing required')
        game = {'source': digest, 'mapper': mapper,
                'submapper': data[8] >> 4 if nes2 else 0,
                'timing': 2 if timing == 3 else timing}
        require(not any(row['game']['source'] == digest for row in games), 'duplicate selected media')
        games.append({'name': entry['name'], 'game': game, 'timing': timing, 'bytes': data})
    return games


def coverage(games):
    require(3 <= len(games) <= 16, 'game coverage outside pair bounds')
    require({game['timing'] for game in games[:3]} == {0, 1, 2}, 'fixture timing coverage differs')
    require(len({game['source'] for game in games}) == len(games), 'duplicate certified media')
    expected = set()
    for game in games[:3]:
        for delay in (0, 2):
            for host in (True, False):
                expected.add((game['source'], delay, host))
    for game in games[3:]:
        for host in (True, False):
            expected.add((game['source'], 2, host))
    return expected
