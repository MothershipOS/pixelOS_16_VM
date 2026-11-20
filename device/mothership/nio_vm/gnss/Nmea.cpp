#include "Nmea.h"

#include <aidl/android/hardware/gnss/GnssConstellationType.h>
#include <utils/SystemClock.h>

#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <ctime>

namespace mship::gnss {

using aidl::android::hardware::gnss::ElapsedRealtime;
using aidl::android::hardware::gnss::GnssConstellationType;
using aidl::android::hardware::gnss::IGnssCallback;

namespace {

std::vector<std::string> split(const std::string& s) {
    std::vector<std::string> f;
    size_t start = 0;
    for (size_t i = 0; i <= s.size(); i++) {
        if (i == s.size() || s[i] == ',') {
            f.push_back(s.substr(start, i - start));
            start = i + 1;
        }
    }
    return f;
}

std::optional<double> num(const std::vector<std::string>& f, size_t i) {
    if (i >= f.size() || f[i].empty()) return std::nullopt;
    char* end = nullptr;
    double v = strtod(f[i].c_str(), &end);
    if (end == f[i].c_str() || !std::isfinite(v)) return std::nullopt;
    return v;
}

// ddmm.mmmm / dddmm.mmmm with its hemisphere -> degrees
std::optional<double> angle(const std::vector<std::string>& f, size_t i, char neg) {
    auto v = num(f, i);
    if (!v || i + 1 >= f.size()) return std::nullopt;
    double deg = std::floor(*v / 100.0);
    double d = deg + (*v - deg * 100.0) / 60.0;
    return f[i + 1] == std::string(1, neg) ? -d : d;
}

// NMEA satellite number within its talker -> Android's constellation, svid and carrier frequency (L1 band)
bool satellite(const std::string& talker, int id, SvInfo* sv) {
    constexpr int64_t L1 = 1575420000, G1 = 1602000000, B1I = 1561098000;
    if (talker == "GP" && id >= 1 && id <= 32) {
        *sv = {.svid = id, .constellation = GnssConstellationType::GPS};
        sv->carrierFrequencyHz = L1;
    } else if (talker == "GP" && id >= 33 && id <= 64) {
        *sv = {.svid = id + 87, .constellation = GnssConstellationType::SBAS};
        sv->carrierFrequencyHz = L1;
    } else if (talker == "GL" && id >= 65 && id <= 96) {
        *sv = {.svid = id - 64, .constellation = GnssConstellationType::GLONASS};
        sv->carrierFrequencyHz = G1;
    } else if (talker == "GA" && id >= 1 && id <= 36) {
        *sv = {.svid = id, .constellation = GnssConstellationType::GALILEO};
        sv->carrierFrequencyHz = L1;
    } else if (talker == "GB" && id >= 1 && id <= 63) {
        *sv = {.svid = id, .constellation = GnssConstellationType::BEIDOU};
        sv->carrierFrequencyHz = B1I;
    } else if (talker == "GQ" && id >= 1 && id <= 10) {
        *sv = {.svid = id + 192, .constellation = GnssConstellationType::QZSS};
        sv->carrierFrequencyHz = L1;
    } else {
        return false;
    }
    return true;
}

// RMC date ddmmyy + time hhmmss.ss -> UTC milliseconds
std::optional<int64_t> utcMs(const std::string& date, const std::string& time) {
    if (date.size() < 6 || time.size() < 6) return std::nullopt;
    struct tm t = {};
    t.tm_mday = atoi(date.substr(0, 2).c_str());
    t.tm_mon = atoi(date.substr(2, 2).c_str()) - 1;
    t.tm_year = atoi(date.substr(4, 2).c_str()) + 100;
    t.tm_hour = atoi(time.substr(0, 2).c_str());
    t.tm_min = atoi(time.substr(2, 2).c_str());
    double sec = atof(time.substr(4).c_str());
    t.tm_sec = static_cast<int>(sec);
    time_t s = timegm(&t);
    if (s < 0) return std::nullopt;
    return static_cast<int64_t>(s) * 1000 + static_cast<int64_t>(std::lround((sec - t.tm_sec) * 1000));
}

bool checksumOk(const std::string& s, std::string* body) {
    if (s.size() < 4 || s[0] != '$') return false;
    size_t star = s.rfind('*');
    if (star == std::string::npos || star + 3 > s.size()) return false;
    unsigned cs = 0;
    for (size_t i = 1; i < star; i++) cs ^= static_cast<unsigned char>(s[i]);
    if (strtoul(s.substr(star + 1, 2).c_str(), nullptr, 16) != cs) return false;
    *body = s.substr(1, star - 1);
    return true;
}

}  // namespace

std::string sentence(const std::string& body) {
    unsigned cs = 0;
    for (char c : body) cs ^= static_cast<unsigned char>(c);
    char tail[8];
    snprintf(tail, sizeof tail, "*%02X\r\n", cs);
    return "$" + body + tail;
}

std::vector<Epoch> Parser::feed(const char* data, size_t n) {
    std::vector<Epoch> out;
    mBuf.append(data, n);
    size_t pos;
    while ((pos = mBuf.find_first_of("\r\n")) != std::string::npos) {
        std::string l = mBuf.substr(0, pos);
        mBuf.erase(0, pos + 1);
        if (!l.empty()) line(l, out);
    }
    if (mBuf.size() > 4096) mBuf.clear();  // no line end: not the protocol
    return out;
}

void Parser::reset() {
    mSats.clear();
    mUsed.clear();
    mSentences.clear();
    mAltMsl = mSeparation = mSigmaLat = mSigmaLon = mSigmaAlt = std::nullopt;
}

void Parser::line(const std::string& s, std::vector<Epoch>& out) {
    std::string body;
    if (!checksumOk(s, &body)) return;
    auto f = split(body);
    if (f.empty() || f[0].size() < 5) return;
    if (f[0] == "PMSHG") {
        if (f.size() >= 3 && f[1] == "STATE") {
            mSlot = f[2] == "ATTACHED" ? Slot::Attached : Slot::Empty;
            if (mSlot == Slot::Empty) reset();
        } else if (f.size() >= 3 && f[1] == "HELLO") {
            mVersion = f[2];
        }
        return;
    }
    mSentences.push_back(s);
    std::string talker = f[0].substr(0, 2), type = f[0].substr(2);
    if (type == "GSV") {
        // count, message, satellites in view, then (id, elevation, azimuth, C/N0) x up to 4, then the signal id
        for (size_t i = 4; i + 3 < f.size(); i += 4) {
            auto id = num(f, i);
            SvInfo sv;
            if (!id || !satellite(talker, static_cast<int>(*id), &sv)) continue;
            sv.cN0Dbhz = static_cast<float>(num(f, i + 3).value_or(0));
            sv.basebandCN0DbHz = sv.cN0Dbhz;
            sv.elevationDegrees = static_cast<float>(num(f, i + 1).value_or(0));
            sv.azimuthDegrees = static_cast<float>(num(f, i + 2).value_or(0));
            sv.svFlag = static_cast<int>(IGnssCallback::GnssSvFlags::HAS_CARRIER_FREQUENCY);
            mSats.push_back(sv);  // USED_IN_FIX comes from the GSA lists, at RMC
        }
    } else if (type == "GSA" && f.size() >= 19) {
        int sys = atoi(f[18].c_str());
        for (size_t i = 3; i < 15; i++) {
            if (auto id = num(f, i)) mUsed.insert({sys, static_cast<int>(*id)});
        }
    } else if (type == "GGA" && f.size() >= 12) {
        mAltMsl = num(f, 9);
        mSeparation = num(f, 11);
    } else if (type == "GST" && f.size() >= 9) {
        mSigmaLat = num(f, 6);
        mSigmaLon = num(f, 7);
        mSigmaAlt = num(f, 8);
    } else if (type == "RMC" && f.size() >= 10) {
        Epoch e;
        e.utcMs = utcMs(f[9], f[1]).value_or(0);
        e.sentences = std::move(mSentences);
        // used: back from Android's numbering to (system id, NMEA number)
        for (auto& sv : mSats) {
            std::pair<int, int> key;
            switch (sv.constellation) {
                case GnssConstellationType::GPS: key = {1, sv.svid}; break;
                case GnssConstellationType::SBAS: key = {1, sv.svid - 87}; break;
                case GnssConstellationType::GLONASS: key = {2, sv.svid + 64}; break;
                case GnssConstellationType::GALILEO: key = {3, sv.svid}; break;
                case GnssConstellationType::BEIDOU: key = {4, sv.svid}; break;
                case GnssConstellationType::QZSS: key = {5, sv.svid - 192}; break;
                default: key = {0, 0};
            }
            if (mUsed.count(key)) {
                sv.svFlag |= static_cast<int>(IGnssCallback::GnssSvFlags::USED_IN_FIX) |
                             static_cast<int>(IGnssCallback::GnssSvFlags::HAS_EPHEMERIS_DATA);
            }
        }
        e.satellites = std::move(mSats);
        auto lat = angle(f, 3, 'S'), lon = angle(f, 5, 'W');
        if (f[2] == "A" && lat && lon) {
            GnssLocation l;
            l.gnssLocationFlags = GnssLocation::HAS_LAT_LONG;
            l.latitudeDegrees = *lat;
            l.longitudeDegrees = *lon;
            if (mAltMsl && mSeparation) {
                l.gnssLocationFlags |= GnssLocation::HAS_ALTITUDE;
                l.altitudeMeters = *mAltMsl + *mSeparation;  // WGS 84 ellipsoid height
            }
            if (auto knots = num(f, 7)) {
                l.gnssLocationFlags |= GnssLocation::HAS_SPEED;
                l.speedMetersPerSec = *knots * 1852.0 / 3600.0;
            }
            if (auto course = num(f, 8)) {
                l.gnssLocationFlags |= GnssLocation::HAS_BEARING;
                l.bearingDegrees = *course;
            }
            if (mSigmaLat && mSigmaLon) {
                l.gnssLocationFlags |= GnssLocation::HAS_HORIZONTAL_ACCURACY;
                l.horizontalAccuracyMeters = std::hypot(*mSigmaLat, *mSigmaLon);
            }
            if (mSigmaAlt) {
                l.gnssLocationFlags |= GnssLocation::HAS_VERTICAL_ACCURACY;
                l.verticalAccuracyMeters = *mSigmaAlt;
            }
            l.timestampMillis = e.utcMs;
            l.elapsedRealtime = {.flags = ElapsedRealtime::HAS_TIMESTAMP_NS | ElapsedRealtime::HAS_TIME_UNCERTAINTY_NS,
                                 .timestampNs = android::elapsedRealtimeNano(),
                                 .timeUncertaintyNs = 1e6};  // the console's delivery: about a millisecond
            e.location = l;
        }
        reset();
        out.push_back(std::move(e));
    }
}

}  // namespace mship::gnss
