import hashlib
from pathlib import Path
import re
import ssl
import sys


REVOKED_FINGERPRINT = "86a1ecba089c4a8d3bbe2734c612ba341d813e043cf9e8a862cd5c57a36bbe6b"


def verify_bundle(path):
    certificates = re.findall(
        r"-----BEGIN CERTIFICATE-----.*?-----END CERTIFICATE-----",
        Path(path).read_text(encoding="ascii"),
        re.DOTALL,
    )
    if len(certificates) < 100:
        raise ValueError("CA bundle is unexpectedly small")
    for certificate in certificates:
        fingerprint = hashlib.sha256(ssl.PEM_cert_to_DER_cert(certificate)).hexdigest()
        if fingerprint == REVOKED_FINGERPRINT:
            raise ValueError("Revoked emSign ECC Root CA - G3 remains in the CA bundle")
    print(f"Verified {len(certificates)} CA certificates; revoked root absent")


if __name__ == "__main__":
    verify_bundle(sys.argv[1])
