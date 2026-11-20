// The mship.gnss 1.x stream (mothershipOS docs/contracts/mship.gnss-1.md): NMEA 0183 epochs from the host, ended
// by RMC, turned into what IGnssCallback reports. No I/O here.
#pragma once

#include <aidl/android/hardware/gnss/GnssLocation.h>
#include <aidl/android/hardware/gnss/IGnssCallback.h>

#include <cstdint>
#include <optional>
#include <set>
#include <string>
#include <utility>
#include <vector>

namespace mship::gnss {

using aidl::android::hardware::gnss::GnssLocation;
using SvInfo = aidl::android::hardware::gnss::IGnssCallback::GnssSvInfo;

struct Epoch {
    // UTC milliseconds of the epoch
    int64_t utcMs = 0;
    std::optional<GnssLocation> location;
    std::vector<SvInfo> satellites;
    // the NMEA sentences as received (without the line end), for gnssNmeaCb
    std::vector<std::string> sentences;
};

enum class Slot { Unknown, Attached, Empty };

class Parser {
  public:
    // Bytes from the port; the epochs they finished.
    std::vector<Epoch> feed(const char* data, size_t n);
    Slot slot() const { return mSlot; }
    // The host's answer to HELLO ("" until it came, "UNSUPPORTED" if no version is shared).
    const std::string& version() const { return mVersion; }

  private:
    void line(const std::string& s, std::vector<Epoch>& out);
    void reset();

    std::string mBuf;
    Slot mSlot = Slot::Unknown;
    std::string mVersion;
    // the epoch so far
    std::vector<SvInfo> mSats;
    std::set<std::pair<int, int>> mUsed;  // (GSA system id, NMEA satellite number)
    std::vector<std::string> mSentences;
    std::optional<double> mAltMsl, mSeparation, mSigmaLat, mSigmaLon, mSigmaAlt;
};

// "$<body>*<checksum>\r\n"
std::string sentence(const std::string& body);

}  // namespace mship::gnss
