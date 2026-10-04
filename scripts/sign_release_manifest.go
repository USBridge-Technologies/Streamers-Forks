// Command sign_release_manifest builds and Ed25519-signs the manifest
// published alongside this release's Sunshine/Punktfunk assets, mirroring
// itsme228/rust-shine's scripts/sign_usbridge_manifest.go and
// USBridge-Remote's scripts/sign_update_manifest.go almost exactly (same
// signing scheme: sign the raw manifest JSON bytes, base64-std-encode the
// signature).
//
// Deliberately signs with USBridge-Remote's existing
// AGENT_UPDATE_ED25519_PRIVATE_KEY keypair rather than minting a new one:
// the agent already embeds and trusts that public key for its own
// self-update manifests (agent/internal/update/pubkey.go), and the agent is
// what fetches these fork releases -- agent/scripts/fetch_sunshine.sh today
// at build time, a runtime launcher later once a fork is only downloaded
// when picked in the UI. One trust anchor instead of two.
//
// No dependencies beyond the Go standard library -- invoked via
// `go run scripts/sign_release_manifest.go ...` straight from
// release-all.yml's `release` job, no module resolution needed.
//
// Usage:
//
//	AGENT_UPDATE_ED25519_PRIVATE_KEY=<base64 ed25519 seed> \
//	  go run scripts/sign_release_manifest.go \
//	    -version v2026.1010.usbridge -dir artifacts \
//	    -manifest-out artifacts/manifest.json \
//	    -sig-out artifacts/manifest.json.sig
//
// -dir is scanned for every known Sunshine/Punktfunk asset filename (see
// knownAssets below, kept in lockstep with release-all.yml's own artifact
// names) that's actually present; each one's exact SHA-256 is hashed
// straight from the file release-all.yml already built, never a value the
// asset itself could disagree with. A missing asset (a partial platform
// run) is just omitted, not an error.
package main

import (
	"crypto/ed25519"
	"crypto/sha256"
	"encoding/base64"
	"encoding/hex"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

type assetEntry struct {
	SHA256 string `json:"sha256"`
}

type manifestDoc struct {
	App     string                `json:"app"`
	Version string                `json:"version"`
	Assets  map[string]assetEntry `json:"assets"`
}

// knownAssets must match the artifact names release-all.yml's build jobs
// upload and the "release" job's `files:` list -- both are a fixed
// packaging convention each build step already hardcodes, not something
// discovered at runtime.
var knownAssets = []string{
	"Sunshine-macOS-arm64.dmg",
	"Sunshine-Windows-x86_64-portable.zip",
	"Sunshine-Linux-x86_64.tar.gz",
	"punktfunk-host-Linux-x86_64.tar.gz",
	"punktfunk-host-Windows-x64.zip",
}

func main() {
	if err := run(); err != nil {
		fmt.Fprintln(os.Stderr, "sign_release_manifest:", err)
		os.Exit(1)
	}
}

func run() error {
	version := flag.String("version", "", "release tag to publish (e.g. v2026.1010.usbridge)")
	dir := flag.String("dir", "", "directory containing the already-downloaded release assets")
	manifestOut := flag.String("manifest-out", "", "path to write the JSON manifest to")
	sigOut := flag.String("sig-out", "", "path to write the base64 Ed25519 signature to")
	keyEnv := flag.String("key-env", "AGENT_UPDATE_ED25519_PRIVATE_KEY", "environment variable holding the base64 Ed25519 private key seed")
	flag.Parse()

	if *version == "" || *dir == "" || *manifestOut == "" || *sigOut == "" {
		return fmt.Errorf("-version, -dir, -manifest-out, and -sig-out are all required")
	}

	seedB64 := os.Getenv(*keyEnv)
	if seedB64 == "" {
		return fmt.Errorf("environment variable %s is empty -- is the GitHub secret wired into this step's env:?", *keyEnv)
	}
	seed, err := base64.StdEncoding.DecodeString(strings.TrimSpace(seedB64))
	if err != nil || len(seed) != ed25519.SeedSize {
		return fmt.Errorf("%s does not decode to a %d-byte Ed25519 seed", *keyEnv, ed25519.SeedSize)
	}
	priv := ed25519.NewKeyFromSeed(seed)

	assets := make(map[string]assetEntry)
	for _, name := range knownAssets {
		p := filepath.Join(*dir, name)
		sum, err := sha256File(p)
		if err != nil {
			if os.IsNotExist(err) {
				continue
			}
			return fmt.Errorf("hash %s: %w", name, err)
		}
		assets[name] = assetEntry{SHA256: sum}
	}
	if len(assets) == 0 {
		return fmt.Errorf("no known release assets found under %s -- refusing to publish an empty manifest", *dir)
	}

	m := manifestDoc{App: "streamers-forks", Version: *version, Assets: assets}
	body, err := json.MarshalIndent(m, "", "  ")
	if err != nil {
		return err
	}
	body = append(body, '\n')

	if err := os.WriteFile(*manifestOut, body, 0o644); err != nil {
		return fmt.Errorf("write manifest: %w", err)
	}

	// Sign the *exact* bytes just written -- the verifier checks the raw
	// release-asset bytes against this signature, so signing anything else
	// (e.g. re-marshaling later) would silently produce a manifest that
	// fails verification for every download.
	sig := ed25519.Sign(priv, body)
	sigB64 := base64.StdEncoding.EncodeToString(sig)
	if err := os.WriteFile(*sigOut, []byte(sigB64+"\n"), 0o644); err != nil {
		return fmt.Errorf("write signature: %w", err)
	}

	keys := make([]string, 0, len(assets))
	for k := range assets {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	fmt.Printf("signed streamers-forks manifest %s: %v -> %s (+ %s)\n", *version, keys, *manifestOut, *sigOut)
	return nil
}

func sha256File(path string) (string, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer f.Close()
	h := sha256.New()
	if _, err := io.Copy(h, f); err != nil {
		return "", err
	}
	return hex.EncodeToString(h.Sum(nil)), nil
}
