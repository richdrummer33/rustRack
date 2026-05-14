#pragma once

#include <atomic>
#include <cstdint>
#include <deque>
#include <functional>
#include <mutex>
#include <string>
#include <thread>
#include <vector>

class BridgeServer {
public:
    BridgeServer() = default;
    ~BridgeServer();

    void start(int port);
    void stop();

    // Broadcasts a payload (any single-line JSON, no trailing newline) to
    // every connected client. The server appends '\n' itself.
    void publish(std::string json);

    // Same as publish() but bypasses the snapshot deduplication: every
    // call enqueues a separate frame, in order. Used for module-asset
    // and action-ack frames where every emission matters.
    void send_one(std::string json);

    // Returns and clears every inbound JSON line received from any client
    // since the last drain. Each entry is one parsed-ready JSON line, no
    // newline. Safe to call from any thread; intended to be drained on
    // the GUI thread.
    std::vector<std::string> drain_inbound();

#ifdef _WIN32
    using socket_t = std::uintptr_t;
#else
    using socket_t = int;
#endif
    static constexpr socket_t kInvalid = (socket_t)-1;

private:
    void run();
    void closeSocket(socket_t s);

    std::atomic<bool> running_{false};
    std::thread thread_;
    int port_ = 0;

    std::mutex snap_mu_;
    std::string latest_;
    std::uint64_t latest_seq_ = 0;

    std::mutex extra_mu_;
    std::deque<std::string> extra_out_;

    std::mutex inbound_mu_;
    std::vector<std::string> inbound_;

    std::vector<socket_t> clients_;
    std::vector<std::uint64_t> client_seq_;
    std::vector<std::string> client_rx_;
};
