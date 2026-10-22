# PUBLIC TEST-ONLY TLS fixtures. Never install this root.

These freshly generated certificates and private key are intentionally public.
They provide no access to an external service. The root is trusted only inside
one test connector / Go RootCAs pool. The server binds IPv4 loopback, with no
external DNS lookup. No certificate/hostname check is disabled.

The server certificate has SAN example.test, 127.0.0.1 and ::1; the CA is name
constrained to example.test/wrong.test and those two loopback IPs, with pathLen0.
The P12 password is the public string `public-test-fixture`. Both certificates
are valid from 2026-10-16 through 2126; long validity prevents fixture clock
expiry from obscuring the DNS-name differential. Their use is restricted to
tests. `wrong.test` has no matching leaf SAN and must fail strict verification.

Generated locally with existing OpenSSL 3.5.7. No downloaded/executed certificate
code or new Rust dependency. The candidate delivery includes exact generation
commands, output identities and certificate validation output. Go/Rust TLS
handshakes remain NOT_RUN until the admitted Windows runner executes them.
