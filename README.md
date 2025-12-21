rapid-control (working title)
=============

This is a MAVLink-based ground station software, for developing and operating autonomous systems (drones, sounding rockets, other equipment). Documentation on MAVLink can be found here: https://mavlink.io/

It is designed to handle scenarios with multiple systems, such as multiple drones, or a rocket and additional equipment (such as a filling station), all communicating separately over potentially separate telemetry links, and supports any system communicating using at least some messages from the `common` MAVLink dialect, and is developed with PX4, Ardupilot and rapid's rocketry firmware in mind, specifically.

# Building & Running

To compile and run, install Rust (preferrably the latest stable release) and run:

```bash
$ cargo run
```

# Usage & Features

TODO: Document which MAVLink message we use for what somewhere

## Multi-Dialect Support

TODO: not implemented

## MAVLink Protocols / "Microservices"

| Protocol / Feature       | Status           |
|--------------------------|------------------|
| Heartbeat                | ✔️ Supported     |
| Mission                  | ❌ Not Supported |
| Parameters               | 🚧 Basic Support |
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

The easiest way to test is with Software-in-the-Loop simulations of firmwares like PX4 & Ardupilot. A docker-compose setup that runs a number of simulations of various vehicle types can be found here: https://github.com/tudsat-rocket/ardupilot-docker
