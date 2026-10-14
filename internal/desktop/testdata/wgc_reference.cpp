// Test-only Windows SDK reference for the same process's exact fixture HWND.
// The caller supplies an initialized apartment. No frame/session, input, window
// discovery, policy changes, or independently selected targets are involved.
// API reference: microsoft/Windows.UI.Composition-Win32-Samples,
// cpp/ScreenCaptureforHWND/ScreenCaptureforHWND/capture.interop.h.
#include <windows.h>
#include <roapi.h>
#include <roerrorapi.h>
#include <restrictederrorinfo.h>
#include <wrl.h>
#include <wrl/wrappers/corewrappers.h>
#include <windows.graphics.capture.h>
#include <windows.graphics.capture.interop.h>
#include <algorithm>
#include <cstddef>
#include <cstdint>

using Microsoft::WRL::ComPtr;
using Microsoft::WRL::Wrappers::HStringReference;
using ABI::Windows::Graphics::Capture::IGraphicsCaptureItem;

struct WgcReferenceResult {
    uint32_t version;
    HRESULT factory, query, clearError, create, extent;
    int32_t width, height;
    GUID itemId;
    HRESULT errorInfo, errorDetails, reportedCode;
    uint32_t hasErrorInfo;
    uint32_t descriptionChars, restrictedChars, capabilityChars;
    wchar_t description[512], restricted[1024], capabilitySid[256];
};
static_assert(sizeof(WgcReferenceResult) == 3660);
static_assert(offsetof(WgcReferenceResult, itemId) == 32);
static_assert(offsetof(WgcReferenceResult, description) == 76);

template <size_t N>
static uint32_t CopyDiagnostic(BSTR source, wchar_t (&destination)[N]) {
    const UINT length = SysStringLen(source);
    if (source != nullptr) {
        const size_t count = (std::min)(static_cast<size_t>(length), N - 1);
        std::copy_n(source, count, destination);
        destination[count] = L'\0';
    }
    return length; // Full length makes any diagnostic truncation explicit.
}

static void ReadRestrictedError(WgcReferenceResult& result) {
    ComPtr<IRestrictedErrorInfo> info;
    result.errorInfo = GetRestrictedErrorInfo(info.GetAddressOf());
    result.hasErrorInfo = info ? 1u : 0u;
    if (!info) return;
    BSTR description = nullptr, restricted = nullptr, capability = nullptr;
    result.errorDetails = info->GetErrorDetails(&description, &result.reportedCode, &restricted, &capability);
    result.descriptionChars = CopyDiagnostic(description, result.description);
    result.restrictedChars = CopyDiagnostic(restricted, result.restricted);
    result.capabilityChars = CopyDiagnostic(capability, result.capabilitySid);
    SysFreeString(description);
    SysFreeString(restricted);
    SysFreeString(capability);
}

extern "C" __declspec(dllexport) HRESULT __stdcall LycheeWGCReference(
    HWND window, WgcReferenceResult* result, uint32_t resultBytes) {
    if (result == nullptr || resultBytes != sizeof(*result)) return E_INVALIDARG;
    *result = {};
    result->version = 1;
    result->itemId = __uuidof(IGraphicsCaptureItem);
    result->factory = result->query = result->clearError = result->create = result->extent = E_PENDING;
    result->errorInfo = result->errorDetails = result->reportedCode = E_PENDING;
    DWORD owner = 0;
    if (GetWindowThreadProcessId(window, &owner) == 0 || owner != GetCurrentProcessId()) return E_ACCESSDENIED;

    ComPtr<IActivationFactory> factory;
    result->factory = RoGetActivationFactory(
        HStringReference(L"Windows.Graphics.Capture.GraphicsCaptureItem").Get(),
        IID_PPV_ARGS(factory.GetAddressOf()));
    if (FAILED(result->factory)) return result->factory;
    ComPtr<IGraphicsCaptureItemInterop> interop;
    result->query = factory.As(&interop);
    if (FAILED(result->query)) return result->query;
    ComPtr<IGraphicsCaptureItem> item;
    {
        ComPtr<IRestrictedErrorInfo> previous;
        result->clearError = GetRestrictedErrorInfo(previous.GetAddressOf());
    } // Retrieval clears stale thread-local error info; release before capture.
    result->create = interop->CreateForWindow(window, __uuidof(IGraphicsCaptureItem), reinterpret_cast<void**>(item.GetAddressOf()));
    if (FAILED(result->create)) {
        // First operation after failure: preserve this apartment's error info
        // before any Release, another COM call, or apartment teardown.
        ReadRestrictedError(*result);
        return result->create;
    }
    ABI::Windows::Graphics::SizeInt32 size{};
    result->extent = item->get_Size(&size);
    result->width = size.Width;
    result->height = size.Height;
    return result->extent;
}
