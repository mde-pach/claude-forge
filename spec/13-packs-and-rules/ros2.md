<!-- Generated from the built-in packs by crates/forge-packs/tests/spec_tables.rs; do not edit. Regenerate: FORGE_BLESS=1 cargo test -p forge-packs --test spec_tables -->

## 13. Packs and rules: `ros2`

### ros2

ROS 2 packages, nodes and launch files.

Detected by: a file matching `**/*.launch.py`; or a line matching `<(buildtool_depend\|depend\|build_depend\|exec_depend)>(ament_cmake\|ament_python\|rclcpp\|rclpy)</` in a file matching `**/package.xml`.

No rules of its own.

| Capability (§20) | Description | Delivered | Relevant when | Active when |
|---|---|---|---|---|
| `ros2/ros2-inspect-running` | Launching a ROS 2 system in the background, reading its output, and inspecting its nodes, topics and QoS. | on-demand | The task needs to observe a running ROS 2 system, such as its launch output, nodes or topics.<br>The task debugs ROS 2 messages that a subscriber misses, or a topic that stays silent. | wherever its pack is |
