// The V4L2 Codec2 component store (external/v4l2_codec2) as the vendor AIDL IComponentStore/default.

#define LOG_TAG "mship-c2-v4l2"

#include <android/binder_manager.h>
#include <android/binder_process.h>
#include <codec2/aidl/ComponentStore.h>
#include <log/log.h>
#include <minijail.h>
#include <signal.h>
#include <v4l2_codec2/v4l2/V4L2ComponentStore.h>

#include <string>

static constexpr char kBaseSeccompPolicyPath[] =
        "/vendor/etc/seccomp_policy/android.hardware.media.c2-default-seccomp_policy";
static constexpr char kExtSeccompPolicyPath[] =
        "/vendor/etc/seccomp_policy/android.hardware.media.c2-extended-seccomp_policy";

int main() {
    signal(SIGPIPE, SIG_IGN);
    android::SetUpMinijail(kBaseSeccompPolicyPath, kExtSeccompPolicyPath);

    ABinderProcess_setThreadPoolMaxThreadCount(16);
    ABinderProcess_startThreadPool();

    using namespace ::aidl::android::hardware::media::c2;
    std::shared_ptr<C2ComponentStore> impl = android::V4L2ComponentStore::Create();
    if (impl == nullptr) {
        ALOGE("cannot create the V4L2 component store");
        return 1;
    }
    std::shared_ptr<IComponentStore> store = ::ndk::SharedRefBase::make<utils::ComponentStore>(impl);
    const std::string name = std::string(IComponentStore::descriptor) + "/default";
    if (AServiceManager_addService(store->asBinder().get(), name.c_str()) != STATUS_OK) {
        ALOGE("cannot register %s", name.c_str());
        return 1;
    }
    ALOGI("%s registered", name.c_str());
    ABinderProcess_joinThreadPool();
    return 0;
}
