nadir
=====

This is a MAVLink-based ground station software, for developing and operating autonomous systems (drones, sounding rockets, other equipment). Documentation on MAVLink can be found here: https://mavlink.io/

It is designed to handle scenarios with multiple systems, such as multiple drones, or a rocket and additional equipment (such as a filling station), all communicating separately over potentially separate telemetry links, and supports any system communicating using at least some messages from the `common` MAVLink dialect, and is developed with PX4, Ardupilot and rapid's rocketry firmware in mind, specifically.

# Building & Running

To compile and run, install Rust and run:

```bash
$ cargo run
```

# Usage & Features

## Multi-Dialect Support

The following MAVLink dialects are supported:

- `common` (used by e.g. PX4)
- `ardupilotmega` (extends `common`, used by Ardupilot)
- `rapid` (extends `common`, used by zenith)

Other dialects which extend `common` will work partially, with non-`common` messages discarded.

## MAVLink Protocols / "Microservices"

| Protocol / Feature       | Status           |
|--------------------------|------------------|
| Heartbeat                | ✔️ Supported     |
| Mission                  | ❌ Not Supported |
| Parameters               | ✔️ Supported     |
| Command                  | 🚧 Basic Support |
| Manual Control / RC      | ❌ Not Supported |
| Camera (v2)              | ❌ Not Supported |
| Gimbal (v2)              | ❌ Not Supported |
| Illuminator              | ❌ Not Supported |
| Offboard Control         | 🚧 Basic Support |
| Battery                  | 🚧 Basic Support |
| Terrain                  | ❌ Not Supported |

See https://mavlink.io/en/services/ for documentation.

## Connection Protocols

By default, the ground station listens for UDP packets on port `14550` and attempts to connect to TCP ports `5760` - `5762` on localhost. Any connected USB serial ports are also opened. (TODO: make configurable)

MAVLink's message signing is currently not supported.

## CAN Bus Forwarding

If the connected system supports the `MAV_CMD_CAN_FORWARD` command, CAN frames forwarded by the system can be inspected and -- if the system accepts `CAN_FRAME` messages sent by the ground station -- manually sent using the ground station software.

On Linux systems, CAN traffic can be forwarded to a local virtual SocketCAN socket to allow other locally running programs to interact with the system's CAN bus. This requires setting up the socket manually as root before starting the ground station software:

```bash
$ sudo modprobe vcan
$ sudo ip link add name vcan0 type vcan
$ sudo ip link set dev vcan0 up
```

# Development

## Testing with Simulation

Development & testing is easiest with software-in-the-loop (SITL) simulation builds of various firmwares:

- For UAVs, you can use this docker-compose setup to test with PX4 and Ardupilot vehicles: https://github.com/tudsat-rocket/ardupilot-docker
- For rockets, use zenith's SITL build target: https://github.com/tudsat-rocket/zenith
