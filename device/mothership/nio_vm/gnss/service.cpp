#define LOG_TAG "gnss-mship"

#include <android/binder_manager.h>
#include <android/binder_process.h>
#include <log/log.h>

#include "Gnss.h"

int main() {
    ABinderProcess_setThreadPoolMaxThreadCount(1);
    ABinderProcess_startThreadPool();
    auto gnss = ndk::SharedRefBase::make<mship::gnss::Gnss>();
    const std::string instance = std::string(mship::gnss::Gnss::descriptor) + "/default";
    if (AServiceManager_addService(gnss->asBinder().get(), instance.c_str()) != STATUS_OK) {
        ALOGE("cannot register %s", instance.c_str());
        return 1;
    }
    ABinderProcess_joinThreadPool();
    return 1;  // not reached
}
