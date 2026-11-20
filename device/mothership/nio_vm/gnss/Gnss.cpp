#define LOG_TAG "gnss-mship"

#include "Gnss.h"

#include <aidl/android/hardware/gnss/GnssSignalType.h>
#include <android-base/properties.h>
#include <fcntl.h>
#include <log/log.h>
#include <termios.h>
#include <unistd.h>

#include <algorithm>
#include <cerrno>
#include <chrono>
#include <cstring>

namespace mship::gnss {

namespace {

constexpr char kPort[] = "/dev/gnss0";
constexpr char kVersions[] = "PMSHG,HELLO,1.0";

ScopedAStatus unsupported() {
    return ScopedAStatus::fromExceptionCode(EX_UNSUPPORTED_OPERATION);
}

}  // namespace

Gnss::Gnss() {
    mReader = std::thread([this] { reader(); });
    mReader.detach();
}

bool Gnss::openPort() {
    std::string port = android::base::GetProperty("vendor.mship.gnss.port", kPort);
    int fd = open(port.c_str(), O_RDWR | O_NOCTTY | O_CLOEXEC);
    if (fd < 0) return false;
    struct termios t;
    if (tcgetattr(fd, &t) == 0) {
        cfmakeraw(&t);  // no echo back to the host, no line editing
        tcsetattr(fd, TCSANOW, &t);
    }
    std::lock_guard<std::mutex> lock(mMutex);
    mFd = fd;
    writeLocked(kVersions);
    sendWantLocked();
    ALOGI("%s open", port.c_str());
    return true;
}

void Gnss::writeLocked(const std::string& body) {
    if (mFd < 0) return;
    std::string s = sentence(body);
    if (write(mFd, s.data(), s.size()) < 0) ALOGW("write: %s", strerror(errno));
}

void Gnss::sendWantLocked() {
    if (mActive) {
        writeLocked("PMSHG,START," + std::to_string(mIntervalMs));
    } else {
        writeLocked("PMSHG,STOP");
    }
}

void Gnss::reader() {
    Parser parser;
    char buf[4096];
    for (;;) {
        if (mFd < 0 && !openPort()) {
            std::this_thread::sleep_for(std::chrono::seconds(2));
            continue;
        }
        ssize_t n = read(mFd, buf, sizeof buf);
        if (n <= 0) {
            if (n < 0 && errno == EINTR) continue;
            ALOGW("read: %s; reopening", n < 0 ? strerror(errno) : "end of file");
            std::lock_guard<std::mutex> lock(mMutex);
            ::close(mFd);
            mFd = -1;
            continue;
        }
        Slot before = parser.slot();
        auto epochs = parser.feed(buf, n);
        if (parser.slot() != before) {
            ALOGI("slot %s", parser.slot() == Slot::Attached ? "attached" : "empty");
            std::lock_guard<std::mutex> lock(mMutex);
            mSlot = parser.slot();
        }
        for (const auto& e : epochs) report(e);
    }
}

void Gnss::report(const Epoch& e) {
    std::shared_ptr<IGnssCallback> cb;
    bool sv, nmea;
    {
        std::lock_guard<std::mutex> lock(mMutex);
        if (!mActive || !mCallback) return;
        cb = mCallback;
        sv = mSvStatus;
        nmea = mNmea;
    }
    if (sv) cb->gnssSvStatusCb(e.satellites);
    if (nmea) {
        for (const auto& s : e.sentences) cb->gnssNmeaCb(e.utcMs, s);
    }
    if (e.location) cb->gnssLocationCb(*e.location);
}

ScopedAStatus Gnss::setCallback(const std::shared_ptr<IGnssCallback>& callback) {
    if (!callback) return ScopedAStatus::fromServiceSpecificError(IGnss::ERROR_INVALID_ARGUMENT);
    {
        std::lock_guard<std::mutex> lock(mMutex);
        mCallback = callback;
    }
    callback->gnssSetCapabilitiesCb(IGnssCallback::CAPABILITY_SCHEDULING);
    callback->gnssSetSystemInfoCb({.yearOfHw = 2026, .name = "mothershipOS mship.gnss"});
    // the L1-band signals the host's receiver tracks
    auto signal = [](GnssConstellationType c, double hz, const char* code) {
        return GnssSignalType{.constellation = c, .carrierFrequencyHz = hz, .codeType = code};
    };
    callback->gnssSetSignalTypeCapabilitiesCb({
            signal(GnssConstellationType::GPS, 1575.42e6, GnssSignalType::CODE_TYPE_C),
            signal(GnssConstellationType::GLONASS, 1602.0e6, GnssSignalType::CODE_TYPE_C),
            signal(GnssConstellationType::GALILEO, 1575.42e6, GnssSignalType::CODE_TYPE_C),
            signal(GnssConstellationType::BEIDOU, 1561.098e6, GnssSignalType::CODE_TYPE_I),
            signal(GnssConstellationType::QZSS, 1575.42e6, GnssSignalType::CODE_TYPE_C),
    });
    return ScopedAStatus::ok();
}

ScopedAStatus Gnss::close() {
    std::lock_guard<std::mutex> lock(mMutex);
    mCallback = nullptr;
    mActive = false;
    sendWantLocked();
    return ScopedAStatus::ok();
}

ScopedAStatus Gnss::start() {
    std::shared_ptr<IGnssCallback> cb;
    {
        std::lock_guard<std::mutex> lock(mMutex);
        mActive = true;
        sendWantLocked();
        cb = mCallback;
    }
    if (cb) {
        cb->gnssStatusCb(IGnssCallback::GnssStatusValue::SESSION_BEGIN);
        cb->gnssStatusCb(IGnssCallback::GnssStatusValue::ENGINE_ON);
    }
    return ScopedAStatus::ok();
}

ScopedAStatus Gnss::stop() {
    std::shared_ptr<IGnssCallback> cb;
    {
        std::lock_guard<std::mutex> lock(mMutex);
        mActive = false;
        sendWantLocked();
        cb = mCallback;
    }
    if (cb) {
        cb->gnssStatusCb(IGnssCallback::GnssStatusValue::SESSION_END);
        cb->gnssStatusCb(IGnssCallback::GnssStatusValue::ENGINE_OFF);
    }
    return ScopedAStatus::ok();
}

ScopedAStatus Gnss::setPositionMode(const IGnss::PositionModeOptions& options) {
    std::lock_guard<std::mutex> lock(mMutex);
    int ms = std::clamp(options.minIntervalMs, 1000, 60000);
    if (ms != mIntervalMs) {
        mIntervalMs = ms;
        if (mActive) sendWantLocked();
    }
    return ScopedAStatus::ok();
}

// The host gives the engine its own time and predicted orbits; the framework's are not needed.
ScopedAStatus Gnss::injectTime(int64_t, int64_t, int32_t) {
    return ScopedAStatus::ok();
}
ScopedAStatus Gnss::injectLocation(const GnssLocation&) {
    return ScopedAStatus::ok();
}
ScopedAStatus Gnss::injectBestLocation(const GnssLocation&) {
    return ScopedAStatus::ok();
}
ScopedAStatus Gnss::deleteAidingData(IGnss::GnssAidingData) {
    return ScopedAStatus::ok();
}

ScopedAStatus Gnss::startSvStatus() {
    std::lock_guard<std::mutex> lock(mMutex);
    mSvStatus = true;
    return ScopedAStatus::ok();
}
ScopedAStatus Gnss::stopSvStatus() {
    std::lock_guard<std::mutex> lock(mMutex);
    mSvStatus = false;
    return ScopedAStatus::ok();
}
ScopedAStatus Gnss::startNmea() {
    std::lock_guard<std::mutex> lock(mMutex);
    mNmea = true;
    return ScopedAStatus::ok();
}
ScopedAStatus Gnss::stopNmea() {
    std::lock_guard<std::mutex> lock(mMutex);
    mNmea = false;
    return ScopedAStatus::ok();
}

ScopedAStatus Gnss::getExtensionPsds(std::shared_ptr<IGnssPsds>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssConfiguration(std::shared_ptr<IGnssConfiguration>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssMeasurement(std::shared_ptr<IGnssMeasurementInterface>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssPowerIndication(std::shared_ptr<IGnssPowerIndication>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssBatching(std::shared_ptr<IGnssBatching>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssGeofence(std::shared_ptr<IGnssGeofence>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssNavigationMessage(std::shared_ptr<IGnssNavigationMessageInterface>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionAGnss(std::shared_ptr<IAGnss>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionAGnssRil(std::shared_ptr<IAGnssRil>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssDebug(std::shared_ptr<IGnssDebug>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssVisibilityControl(std::shared_ptr<visibility_control::IGnssVisibilityControl>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssAntennaInfo(std::shared_ptr<IGnssAntennaInfo>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionMeasurementCorrections(
        std::shared_ptr<measurement_corrections::IMeasurementCorrectionsInterface>*) {
    return unsupported();
}
ScopedAStatus Gnss::getExtensionGnssAssistanceInterface(std::shared_ptr<gnss_assistance::IGnssAssistanceInterface>*) {
    return unsupported();
}

}  // namespace mship::gnss
