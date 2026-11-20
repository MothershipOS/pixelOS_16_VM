// IGnss for the mship.gnss slot: the phone's real receiver, owned by the mothershipOS host, as NMEA epochs on
// /dev/gnss0 (Cuttlefish's GNSS console, hvc6). Positions, accuracies and satellites only: the extensions
// (measurements, PSDS, batching, geofencing, ...) are declined, so the framework leaves them out.
#pragma once

#include <aidl/android/hardware/gnss/BnGnss.h>

#include <atomic>
#include <memory>
#include <mutex>
#include <string>
#include <thread>

#include "Nmea.h"

namespace mship::gnss {

using namespace aidl::android::hardware::gnss;
using ndk::ScopedAStatus;

class Gnss : public BnGnss {
  public:
    Gnss();

    ScopedAStatus setCallback(const std::shared_ptr<IGnssCallback>& callback) override;
    ScopedAStatus close() override;
    ScopedAStatus start() override;
    ScopedAStatus stop() override;
    ScopedAStatus injectTime(int64_t timeMs, int64_t timeReferenceMs, int32_t uncertaintyMs) override;
    ScopedAStatus injectLocation(const GnssLocation& location) override;
    ScopedAStatus injectBestLocation(const GnssLocation& location) override;
    ScopedAStatus deleteAidingData(IGnss::GnssAidingData flags) override;
    ScopedAStatus setPositionMode(const IGnss::PositionModeOptions& options) override;
    ScopedAStatus startSvStatus() override;
    ScopedAStatus stopSvStatus() override;
    ScopedAStatus startNmea() override;
    ScopedAStatus stopNmea() override;

    ScopedAStatus getExtensionPsds(std::shared_ptr<IGnssPsds>* out) override;
    ScopedAStatus getExtensionGnssConfiguration(std::shared_ptr<IGnssConfiguration>* out) override;
    ScopedAStatus getExtensionGnssMeasurement(std::shared_ptr<IGnssMeasurementInterface>* out) override;
    ScopedAStatus getExtensionGnssPowerIndication(std::shared_ptr<IGnssPowerIndication>* out) override;
    ScopedAStatus getExtensionGnssBatching(std::shared_ptr<IGnssBatching>* out) override;
    ScopedAStatus getExtensionGnssGeofence(std::shared_ptr<IGnssGeofence>* out) override;
    ScopedAStatus getExtensionGnssNavigationMessage(std::shared_ptr<IGnssNavigationMessageInterface>* out) override;
    ScopedAStatus getExtensionAGnss(std::shared_ptr<IAGnss>* out) override;
    ScopedAStatus getExtensionAGnssRil(std::shared_ptr<IAGnssRil>* out) override;
    ScopedAStatus getExtensionGnssDebug(std::shared_ptr<IGnssDebug>* out) override;
    ScopedAStatus getExtensionGnssVisibilityControl(
            std::shared_ptr<visibility_control::IGnssVisibilityControl>* out) override;
    ScopedAStatus getExtensionGnssAntennaInfo(std::shared_ptr<IGnssAntennaInfo>* out) override;
    ScopedAStatus getExtensionMeasurementCorrections(
            std::shared_ptr<measurement_corrections::IMeasurementCorrectionsInterface>* out) override;
    ScopedAStatus getExtensionGnssAssistanceInterface(
            std::shared_ptr<gnss_assistance::IGnssAssistanceInterface>* out) override;

  private:
    // the port: opened raw, HELLO, then what the framework wants; reopened if it fails
    void reader();
    bool openPort();
    // START/STOP for the current state (with mMutex held)
    void sendWantLocked();
    void writeLocked(const std::string& body);
    void report(const Epoch& e);

    std::mutex mMutex;
    std::shared_ptr<IGnssCallback> mCallback;
    int mFd = -1;
    bool mActive = false;
    bool mSvStatus = true;
    bool mNmea = true;
    int mIntervalMs = 1000;
    Slot mSlot = Slot::Unknown;
    std::thread mReader;
};

}  // namespace mship::gnss
