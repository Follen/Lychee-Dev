// SPDX-License-Identifier: AGPL-3.0-or-later
// AVI demuxing adapted from wowdata; see THIRD_PARTY_NOTICES.md.
package video

import (
	"encoding/binary"
	"errors"
)

var (
	// ErrContainerFormat reports structurally invalid RIFF/AVI input.
	ErrContainerFormat = errors.New("records.avi_invalid")
	// ErrContainerTruncated reports input that ends inside declared extents.
	ErrContainerTruncated = errors.New("records.avi_truncated")
	// ErrMoviMissing reports a valid AVI container without a movi list.
	ErrMoviMissing = errors.New("records.avi_movi_missing")
	// ErrFrameLimit reports a bound violation that must never be papered over.
	ErrFrameLimit = errors.New("records.frame_limit_exceeded")
)

// Frame preserves the legacy demux record fields (type/timestamp/duration in
// microseconds, size in bytes) and adds the exact chunk identity and payload
// needed for bounded frame output. Timestamp and duration use the legacy
// integer-truncated microsecond arithmetic so metadata stays comparable.
type Frame struct {
	Type      string  `json:"type"`
	Timestamp float64 `json:"timestamp"`
	Duration  float64 `json:"duration"`
	Size      int     `json:"size"`
	Chunk     string  `json:"chunk"`
	Offset    int64   `json:"offset"`
	Payload   []byte  `json:"-"`
	SubFrames []int   `json:"subFrames,omitempty"`
}

// Container is one parsed VP9-in-AVI presentation. Complete is false whenever
// enumeration stopped at a bound; TruncationReason names the bound that hit.
type Container struct {
	Width            int      `json:"width"`
	Height           int      `json:"height"`
	FrameRate        float64  `json:"frameRate"`
	Frames           []Frame  `json:"frames"`
	FrameCount       int      `json:"frameCount"`
	EnumeratedFrames int      `json:"enumeratedFrames"`
	EnumeratedBytes  int64    `json:"enumeratedBytes"`
	Complete         bool     `json:"complete"`
	Truncated        bool     `json:"truncated"`
	TruncationReason string   `json:"truncationReason,omitempty"`
	Warnings         []string `json:"warnings,omitempty"`
}

// Limits bound enumeration and payload handling. Zero values select bounded
// defaults; a single frame above MaxFrameBytes always fails with ErrFrameLimit.
type Limits struct {
	MaxFrames     int
	MaxFrameBytes int64
	MaxTotalBytes int64
	AllowPartial  bool
}

const (
	defaultMaxFrames     = 100000
	defaultMaxFrameBytes = 256 << 20
	defaultMaxTotalBytes = 1 << 30
	headerListMaxDepth   = 16
)

func normalizeLimits(limits Limits) Limits {
	if limits.MaxFrames <= 0 {
		limits.MaxFrames = defaultMaxFrames
	}
	if limits.MaxFrameBytes <= 0 {
		limits.MaxFrameBytes = defaultMaxFrameBytes
	}
	if limits.MaxTotalBytes <= 0 {
		limits.MaxTotalBytes = defaultMaxTotalBytes
	}
	return limits
}

// ParseAVI parses a RIFF/AVI container with VP9 (or any) video chunks,
// enumerates the "00dc"/"00db" video chunks of the movi list and returns their
// frame records with payloads. Structural violations fail precisely; a frame
// sequence longer than the limits is truncated visibly when AllowPartial is
// set and otherwise fails with ErrFrameLimit.
func ParseAVI(data []byte, limits Limits) (*Container, error) {
	limits = normalizeLimits(limits)
	fileSize := int64(len(data))
	if fileSize < 12 {
		return nil, ErrContainerTruncated
	}
	if string(data[0:4]) != "RIFF" {
		return nil, ErrContainerFormat
	}
	if string(data[8:12]) != "AVI " {
		return nil, ErrContainerFormat
	}
	result := &Container{FrameRate: 30.0}
	riffSize := int64(binary.LittleEndian.Uint32(data[4:8]))
	if riffSize+8 > fileSize {
		return nil, ErrContainerTruncated
	}
	if riffSize+8 != fileSize {
		// Legacy parsing walks to the physical end of file; keep that and make
		// the disagreement visible instead of failing on real samples.
		result.Warnings = append(result.Warnings, "riff_size_mismatch")
	}
	moviStart, moviEnd := int64(-1), int64(-1)
	pos := int64(12)
	for pos+8 <= fileSize {
		chunk, err := readChunkHeader(data, pos, fileSize)
		if err != nil {
			return nil, err
		}
		switch chunk.id {
		case "LIST":
			if chunk.size < 4 {
				return nil, ErrContainerFormat
			}
			listType := string(data[pos+8 : pos+12])
			if listType == "movi" {
				if moviStart < 0 {
					moviStart, moviEnd = pos+12, chunk.end
				}
			} else if err := result.scanHeaderList(data, pos+12, chunk.end, 1); err != nil {
				return nil, err
			}
		case "movi":
			// Legacy also accepts a bare movi chunk without a LIST wrapper.
			if moviStart < 0 {
				moviStart, moviEnd = pos+8, chunk.end
			}
		case "avih":
			if chunk.size < 4 {
				return nil, ErrContainerFormat
			}
			usPerFrame := binary.LittleEndian.Uint32(data[pos+8 : pos+12])
			if usPerFrame > 0 {
				result.FrameRate = 1000000.0 / float64(usPerFrame)
			}
		}
		pos = chunk.next(fileSize)
	}
	if moviStart < 0 {
		return nil, ErrMoviMissing
	}
	if err := result.enumerateFrames(data, moviStart, moviEnd, limits); err != nil {
		return nil, err
	}
	result.FrameCount = len(result.Frames)
	result.Complete = !result.Truncated
	return result, nil
}

type chunkHeader struct {
	id   string
	size int64
	end  int64
}

func readChunkHeader(data []byte, pos, bound int64) (chunkHeader, error) {
	if pos < 0 || pos+8 > bound {
		return chunkHeader{}, ErrContainerTruncated
	}
	size := int64(binary.LittleEndian.Uint32(data[pos+4 : pos+8]))
	end := pos + 8 + size
	if end < pos || end > bound {
		return chunkHeader{}, ErrContainerTruncated
	}
	return chunkHeader{id: string(data[pos : pos+4]), size: size, end: end}, nil
}

// next advances past a chunk payload and its RIFF pad byte. A missing pad
// byte on the very last chunk is tolerated.
func (c chunkHeader) next(bound int64) int64 {
	next := c.end + c.size&1
	if c.size&1 == 1 && next > bound && c.end == bound {
		return bound
	}
	return next
}

// scanHeaderList walks one header list for avih/strf chunks with bounded
// recursion. strf records shorter than 40 bytes (audio formats) are ignored.
func (c *Container) scanHeaderList(data []byte, pos, end int64, depth int) error {
	if depth > headerListMaxDepth {
		return ErrContainerFormat
	}
	for pos+8 <= end {
		chunk, err := readChunkHeader(data, pos, end)
		if err != nil {
			return err
		}
		switch chunk.id {
		case "LIST":
			if chunk.size < 4 {
				return ErrContainerFormat
			}
			if err := c.scanHeaderList(data, pos+12, chunk.end, depth+1); err != nil {
				return err
			}
		case "avih":
			if chunk.size < 4 {
				return ErrContainerFormat
			}
			usPerFrame := binary.LittleEndian.Uint32(data[pos+8 : pos+12])
			if usPerFrame > 0 {
				c.FrameRate = 1000000.0 / float64(usPerFrame)
			}
		case "strf":
			// BITMAPINFOHEADER: biWidth at +4, biHeight at +8. The legacy
			// reader keeps the last video-format record it sees; identical
			// selection is preserved here.
			if chunk.size >= 40 {
				c.Width = int(binary.LittleEndian.Uint32(data[pos+12 : pos+16]))
				c.Height = int(binary.LittleEndian.Uint32(data[pos+16 : pos+20]))
			}
		}
		pos = chunk.next(end)
	}
	return nil
}

// enumerateFrames walks the movi list and records every video chunk with the
// legacy timestamp/duration arithmetic. Enumeration keeps counting past a
// truncation bound so the manifest can show how much was left behind.
func (c *Container) enumerateFrames(data []byte, start, end int64, limits Limits) error {
	frameDuration := 0.0
	if c.FrameRate > 0 {
		frameDuration = float64(int(1000000.0 / c.FrameRate))
	}
	timestamp := 0.0
	var keptBytes int64
	pos := start
	for pos+8 <= end {
		chunk, err := readChunkHeader(data, pos, end)
		if err != nil {
			return err
		}
		if chunk.id != "00dc" && chunk.id != "00db" {
			pos = chunk.next(end)
			continue
		}
		if chunk.size > limits.MaxFrameBytes {
			return ErrFrameLimit
		}
		c.EnumeratedFrames++
		c.EnumeratedBytes += chunk.size
		if c.Truncated {
			// Only counting the remainder after a visible truncation.
			pos = chunk.next(end)
			continue
		}
		reason := ""
		if len(c.Frames) >= limits.MaxFrames {
			reason = "frame_limit"
		} else if keptBytes+chunk.size > limits.MaxTotalBytes {
			reason = "byte_limit"
		}
		if reason != "" {
			if !limits.AllowPartial {
				return ErrFrameLimit
			}
			c.Truncated = true
			c.TruncationReason = reason
			pos = chunk.next(end)
			continue
		}
		payload := data[pos+8 : chunk.end]
		frame := Frame{
			Type:      "key",
			Timestamp: timestamp,
			Duration:  frameDuration,
			Size:      int(chunk.size),
			Chunk:     chunk.id,
			Offset:    pos + 8,
			Payload:   payload,
			SubFrames: SuperframeSizes(payload),
		}
		c.Frames = append(c.Frames, frame)
		keptBytes += chunk.size
		timestamp += frameDuration
		pos = chunk.next(end)
	}
	return nil
}

// SuperframeSizes reports the VP9 sub-frame byte sizes when the payload ends
// with a valid superframe index (single-frame payloads and unverifiable
// indexes return nil). The index is accepted only when its marker bookends
// match, the declared sizes cover the payload exactly, and any duplicated size
// table agrees, so a false positive cannot invent frame boundaries.
func SuperframeSizes(payload []byte) []int {
	n := len(payload)
	if n < 2 {
		return nil
	}
	marker := payload[n-1]
	if marker&0xE0 != 0xC0 {
		return nil
	}
	frames := int(marker&0x07) + 1
	if frames == 1 {
		return nil
	}
	mag := int((marker>>3)&0x03) + 1
	for _, tableCopies := range []int{1, 2} {
		for _, bookend := range []int{0, 1} {
			indexSize := bookend + tableCopies*frames*mag + 1
			if indexSize > n {
				continue
			}
			start := n - indexSize
			if bookend == 1 && payload[start] != marker {
				continue
			}
			sizes := make([]int, 0, frames)
			pos := start + bookend
			sum := 0
			valid := true
			for table := 0; table < tableCopies && valid; table++ {
				for i := 0; i < frames; i++ {
					value := 0
					for b := 0; b < mag; b++ {
						value |= int(payload[pos+b]) << (8 * b)
					}
					pos += mag
					if value == 0 {
						valid = false
						break
					}
					if table == 0 {
						sizes = append(sizes, value)
						sum += value
					} else if sizes[i] != value {
						valid = false
						break
					}
				}
			}
			if valid && sum == start {
				return sizes
			}
		}
	}
	return nil
}
