//go:build windows && amd64

package desktop

import (
	"context"
	"errors"
	"fmt"
	"image"
	"runtime"
	"sync"
	"sync/atomic"
	"time"
	"unsafe"

	"golang.org/x/sys/windows"
)

var (
	d3dLibrary         = windows.NewLazySystemDLL("d3d11.dll")
	createDevice       = d3dLibrary.NewProc("D3D11CreateDevice")
	wrapDevice         = d3dLibrary.NewProc("CreateDirect3D11DeviceFromDXGIDevice")
	dxgiDeviceID       = guid("54ec77fa-1377-44e6-8c32-88fd5f44c84c")
	runtimeDeviceID    = guid("a37624ab-8d5f-4650-9d3e-9eae3d9bc670")
	framePoolFactoryID = guid("589b103f-6bbc-5df5-a991-02e28b3b66d5")
	closableID         = guid("30d5a829-7fa4-4026-83bb-d75bae4ea99e")
	surfaceAccessID    = guid("a9b3d012-3df2-4ee3-b8d1-8695f457d3c1")
	textureID          = guid("6f15aaf2-d208-4e89-9ab4-489535d34f9c")
	unknownID          = guid("00000000-0000-0000-c000-000000000046")
	agileID            = guid("94ea2b94-e9cc-49e0-c0ff-ee64ca8f5b90")
	frameHandlerID     = guid("51a947f7-79cf-5a3e-a3a5-1289cfa6dfe8")
	captureRateID      = guid("67c0ea62-1f85-5061-925a-239be0ac09cb")
	activeNotices      sync.Map
	noticeVTable       = [4]uintptr{
		windows.NewCallback(func(n *frameNotice, id *windows.GUID, out **frameNotice) uintptr {
			if out == nil {
				return 0x80004003
			}
			*out = nil
			if id != nil && (*id == unknownID || *id == agileID || *id == frameHandlerID) {
				n.refs.Add(1)
				*out = n
				return 0
			}
			return 0x80004002
		}),
		windows.NewCallback(func(n *frameNotice) uintptr { return uintptr(n.refs.Add(1)) }),
		windows.NewCallback(func(n *frameNotice) uintptr { return n.drop() }),
		windows.NewCallback(func(n *frameNotice, _ *nativeInterface, _ *nativeInterface) uintptr {
			select {
			case n.arrived <- struct{}{}:
			default:
			}
			return 0
		}),
	}
)

func guid(text string) windows.GUID {
	value, err := windows.GUIDFromString("{" + text + "}")
	if err != nil {
		panic(err)
	}
	return value
}
func (n *nativeInterface) query(id *windows.GUID) (*nativeInterface, error) {
	var result *nativeInterface
	err := runtimeFailure("query_interface", n.call(0, uintptr(unsafe.Pointer(id)), uintptr(unsafe.Pointer(&result))))
	return result, err
}
func (n *nativeInterface) closeRuntime() {
	if n == nil {
		return
	}
	closer, err := n.query(&closableID)
	if err == nil {
		closer.call(6)
		closer.release()
	}
}

// A single callback thunk per method is reused across every session. COM-held
// delegates stay rooted and pinned until their final native Release.
type frameNotice struct {
	vtable  *[4]uintptr
	refs    atomic.Int32
	arrived chan struct{}
	pin     runtime.Pinner
}

func newFrameNotice() *frameNotice {
	n := &frameNotice{vtable: &noticeVTable, arrived: make(chan struct{}, 1)}
	n.refs.Store(1)
	n.pin.Pin(n)
	activeNotices.Store(n, true)
	return n
}
func (n *frameNotice) drop() uintptr {
	remaining := n.refs.Add(-1)
	if remaining == 0 {
		activeNotices.Delete(n)
		n.pin.Unpin()
	}
	return uintptr(remaining)
}

// A zero rectangle selects a bounded top-left receipt region of the resolved
// window. CaptureArea returns its exact coordinates after native extent validation.
func CaptureFrames(parent context.Context, target WindowIdentity, roi image.Rectangle) (*FrameStream, error) {
	if err := ConfirmWindow(parent, target); err != nil {
		return nil, err
	}
	if roi != (image.Rectangle{}) && roi != WholeWindowCapture() && (roi.Empty() || roi.Min.X < 0 || roi.Min.Y < 0 || roi.Dx() > 4096 || roi.Dy() > 4096) {
		return nil, errors.New("desktop.invalid_capture_region")
	}
	if err := retainCaptureRuntime(); err != nil {
		return nil, err
	}
	ctx, cancel := context.WithCancel(parent)
	frames := make(chan *CapturedFrame, 1)
	failures := make(chan error, 1)
	done := make(chan struct{})
	ready := make(chan error, 1)
	s := &FrameStream{Frames: frames, Errors: failures, cancel: cancel, done: done}
	go func() {
		defer close(done)
		defer close(frames)
		defer close(failures)
		runtime.LockOSThread()
		defer runtime.UnlockOSThread()
		code, _, _ := initializeRuntime.Call(1)
		if err := runtimeFailure("initialize_runtime", code); err != nil {
			ready <- err
			return
		}
		defer finishRuntime.Call()
		capture, err := startFrameCapture(target, roi)
		if err != nil {
			ready <- err
			return
		}
		defer capture.close()
		roi, err = ResolveCaptureArea(roi, image.Pt(int(capture.extent.Width), int(capture.extent.Height)))
		if err != nil {
			ready <- err
			return
		}
		s.area = roi
		ready <- nil
		var lastCopy time.Time
		for {
			select {
			case <-ctx.Done():
				return
			case <-capture.notice.arrived:
				if err := ConfirmWindow(ctx, target); err != nil {
					failures <- err
					return
				}
				frame, err := capture.readRegion(roi, time.Since(lastCopy) >= 100*time.Millisecond)
				if err != nil {
					failures <- err
					return
				}
				if frame == nil {
					continue
				}
				lastCopy = time.Now()
				select {
				case frames <- frame:
				default:
					select {
					case <-frames:
					default:
					}
					select {
					case frames <- frame:
					case <-ctx.Done():
						return
					}
				}
			}
		}
	}()
	select {
	case err := <-ready:
		if err != nil {
			cancel()
			<-done
			return nil, err
		}
		return s, nil
	case <-ctx.Done():
		cancel()
		<-done
		return nil, ctx.Err()
	}
}

type frameCapture struct {
	device, immediate, wrapped, item, pool, session, staging *nativeInterface
	notice                                                   *frameNotice
	token                                                    int64
	registered                                               bool
	width, height                                            uint32
	extent                                                   captureExtent
}

func (c *frameCapture) close() {
	c.session.closeRuntime()
	if c.registered {
		c.pool.call(9, uintptr(c.token))
	}
	c.pool.closeRuntime()
	if c.notice != nil {
		c.notice.drop()
	}
	for _, ref := range []*nativeInterface{c.staging, c.session, c.pool, c.item, c.wrapped, c.immediate, c.device} {
		ref.release()
	}
}
func startFrameCapture(target WindowIdentity, roi image.Rectangle) (result *frameCapture, err error) {
	c := &frameCapture{}
	defer func() {
		if err != nil {
			c.close()
		}
	}()
	code, _, _ := createDevice.Call(0, 1, 0, 0x20, 0, 0, 7, uintptr(unsafe.Pointer(&c.device)), 0, uintptr(unsafe.Pointer(&c.immediate)))
	if err = runtimeFailure("create_d3d_device", code); err != nil {
		return nil, err
	}
	dxgi, err := c.device.query(&dxgiDeviceID)
	if err != nil {
		return nil, err
	}
	defer dxgi.release()
	var inspectable *nativeInterface
	code, _, _ = wrapDevice.Call(uintptr(unsafe.Pointer(dxgi)), uintptr(unsafe.Pointer(&inspectable)))
	if err = runtimeFailure("wrap_d3d_device", code); err != nil {
		return nil, err
	}
	defer inspectable.release()
	c.wrapped, err = inspectable.query(&runtimeDeviceID)
	if err != nil {
		return nil, err
	}
	var extent captureExtent
	c.item, extent, err = openCaptureItem(uintptr(target.Handle))
	if err != nil {
		return nil, err
	}
	if _, err := ResolveCaptureArea(roi, image.Pt(int(extent.Width), int(extent.Height))); err != nil {
		return nil, err
	}
	c.extent = extent
	factory, err := activationFactory("Windows.Graphics.Capture.Direct3D11CaptureFramePool", &framePoolFactoryID)
	if err != nil {
		return nil, err
	}
	defer factory.release()
	// SizeInt32 is an eight-byte value passed directly in the Win64 ABI.
	packedSize := uintptr(uint64(uint32(extent.Width)) | uint64(uint32(extent.Height))<<32)
	err = runtimeFailure("create_frame_pool", factory.call(6, uintptr(unsafe.Pointer(c.wrapped)), 87, 1, packedSize, uintptr(unsafe.Pointer(&c.pool))))
	if err != nil {
		return nil, err
	}
	c.notice = newFrameNotice()
	err = runtimeFailure("subscribe_frame", c.pool.call(8, uintptr(unsafe.Pointer(c.notice)), uintptr(unsafe.Pointer(&c.token))))
	if err != nil {
		return nil, err
	}
	c.registered = true
	err = runtimeFailure("create_capture_session", c.pool.call(10, uintptr(unsafe.Pointer(c.item)), uintptr(unsafe.Pointer(&c.session))))
	if err != nil {
		return nil, err
	}
	// Newer Windows can throttle the producer. Older supported Windows still
	// drains events, but copies CPU pixels at most ten times per second.
	if rate, rateErr := c.session.query(&captureRateID); rateErr == nil {
		rate.call(7, 1000000)
		rate.release()
	}
	err = runtimeFailure("start_capture", c.session.call(6))
	if err != nil {
		return nil, err
	}
	return c, nil
}

type textureDescription struct{ Width, Height, MipLevels, ArraySize, Format, SampleCount, SampleQuality, Usage, BindFlags, CPUAccessFlags, MiscFlags uint32 }
type mappedTexture struct {
	Data                 unsafe.Pointer
	RowPitch, DepthPitch uint32
}

type textureRegion struct{ Left, Top, Front, Right, Bottom, Back uint32 }

func (c *frameCapture) readRegion(roi image.Rectangle, copyPixels bool) (*CapturedFrame, error) {
	var frame *nativeInterface
	if err := runtimeFailure("next_frame", c.pool.call(7, uintptr(unsafe.Pointer(&frame)))); err != nil {
		return nil, err
	}
	if frame == nil {
		return nil, nil
	}
	defer frame.release()
	defer frame.closeRuntime()
	if !copyPixels {
		return nil, nil
	}
	var extent captureExtent
	if err := runtimeFailure("frame_extent", frame.call(8, uintptr(unsafe.Pointer(&extent)))); err != nil {
		return nil, err
	}
	if extent != c.extent {
		return nil, errors.New("desktop.capture_resized")
	}
	var ticks int64
	if err := runtimeFailure("frame_time", frame.call(7, uintptr(unsafe.Pointer(&ticks)))); err != nil {
		return nil, err
	}
	var surface *nativeInterface
	if err := runtimeFailure("frame_surface", frame.call(6, uintptr(unsafe.Pointer(&surface)))); err != nil {
		return nil, err
	}
	defer surface.release()
	access, err := surface.query(&surfaceAccessID)
	if err != nil {
		return nil, err
	}
	defer access.release()
	var texture *nativeInterface
	if err := runtimeFailure("surface_texture", access.call(3, uintptr(unsafe.Pointer(&textureID)), uintptr(unsafe.Pointer(&texture)))); err != nil {
		return nil, err
	}
	defer texture.release()
	var desc textureDescription
	texture.call(10, uintptr(unsafe.Pointer(&desc)))
	if desc.Format != 87 || desc.SampleCount != 1 || desc.Width > 16384 || desc.Height > 16384 || desc.Width < uint32(roi.Max.X) || desc.Height < uint32(roi.Max.Y) {
		return nil, errors.New("desktop.unsupported_capture_texture")
	}
	if c.staging == nil || c.width != uint32(roi.Dx()) || c.height != uint32(roi.Dy()) {
		c.staging.release()
		c.staging = nil
		desc.Usage = 3
		desc.BindFlags = 0
		desc.CPUAccessFlags = 0x20000
		desc.MiscFlags = 0
		desc.Width = uint32(roi.Dx())
		desc.Height = uint32(roi.Dy())
		if err := runtimeFailure("create_staging_texture", c.device.call(5, uintptr(unsafe.Pointer(&desc)), 0, uintptr(unsafe.Pointer(&c.staging)))); err != nil {
			return nil, err
		}
		c.width = desc.Width
		c.height = desc.Height
	}
	region := textureRegion{Left: uint32(roi.Min.X), Top: uint32(roi.Min.Y), Right: uint32(roi.Max.X), Bottom: uint32(roi.Max.Y), Back: 1}
	c.immediate.call(46, uintptr(unsafe.Pointer(c.staging)), 0, 0, 0, 0, uintptr(unsafe.Pointer(texture)), 0, uintptr(unsafe.Pointer(&region)))
	var mapped mappedTexture
	if err := runtimeFailure("map_capture_texture", c.immediate.call(14, uintptr(unsafe.Pointer(c.staging)), 0, 1, 0, uintptr(unsafe.Pointer(&mapped)))); err != nil {
		return nil, err
	}
	defer c.immediate.call(15, uintptr(unsafe.Pointer(c.staging)), 0)
	if mapped.Data == nil || mapped.RowPitch < uint32(roi.Dx())*4 || mapped.RowPitch > 1<<20 {
		return nil, fmt.Errorf("desktop.invalid_texture_pitch: %d", mapped.RowPitch)
	}
	pixels := image.NewNRGBA(image.Rect(0, 0, roi.Dx(), roi.Dy()))
	for y := 0; y < roi.Dy(); y++ {
		source := unsafe.Slice((*byte)(unsafe.Add(mapped.Data, uintptr(y)*uintptr(mapped.RowPitch))), roi.Dx()*4)
		target := pixels.Pix[y*pixels.Stride : (y+1)*pixels.Stride]
		for x := 0; x < len(target); x += 4 {
			target[x] = source[x+2]
			target[x+1] = source[x+1]
			target[x+2] = source[x]
			target[x+3] = 255
		}
	}
	return &CapturedFrame{NRGBA: pixels, SystemTicks: ticks, ObservedAt: time.Now()}, nil
}
