//! 启动编排。
//!
//! 从 virtio-blk 上的 MINIX 文件系统按路径读取用户程序 ELF，并交给
//! `task::exec::exec_buffer` 加载执行。

/// 启动系统内置的用户程序。
pub(crate) fn start_default_programs() {
    for path in ["/bin/init", "/bin/fork", "/bin/mmap_test"] {
        crate::binfmt::route(path, &[path], &[]).unwrap();
    }

    let envp = [
        "MOTD_SHOWN=pam",
        "GDM_LANG=zh_CN.UTF-8",
        "QT_IM_MODULES=wayland;ibus",
        "LOGNAME=netsalmon",
        "XDG_RUNTIME_DIR=/run/user/1000",
        "GNOME_SETUP_DISPLAY=unix:/tmp/.X11-unix/X1",
        "STARSHIP_SESSION_KEY=2675210427793522",
        "INVOCATION_ID=4b9bece284bb43ea9147fca4ff71d102",
        "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus",
        "XDG_SESSION_EXTRA_DEVICE_ACCESS=render:accel",
        "SSH_AUTH_SOCK=/run/user/1000/gcr/ssh",
        "STARSHIP_SHELL=fish",
        "XMODIFIERS=@im=ibus",
        "HOME=/home/netsalmon",
        "PWD=/home/netsalmon/Projects/athera",
        "TERM=xterm-256color",
        "MAIL=/var/spool/mail/netsalmon",
        "MANAGERPIDFDID=61523",
        "JOURNAL_STREAM=10:27902",
        "XDG_SESSION_DESKTOP=gnome",
        "TERM_SESSION_ID=d61463ba-6ec2-443d-b78f-61712c56776e",
        "XDG_SESSION_CLASS=user",
        "GIO_LAUNCHED_DESKTOP_FILE_PID=35156",
        "USER=netsalmon",
        "DESKTOP_SESSION=gnome",
        "MANAGERPID=3084",
        "QT_IM_MODULE=ibus",
        "GJS_DEBUG_OUTPUT=stderr",
        "SHLVL=1",
        "TERMINAL_EMULATOR=JetBrains-JediTerm",
        "XAUTHORITY=/run/user/1000/.mutter-Xwaylandauth.4MPFU3",
        "GDMSESSION=gnome",
        "SYSTEMD_EXEC_PID=3222",
        "XDG_CURRENT_DESKTOP=GNOME",
        "DISPLAY=:0",
        "LANG=zh_CN.UTF-8",
        "GJS_DEBUG_TOPICS=JS ERROR;JS LOG",
        "XDG_SESSION_TYPE=wayland",
        "USERNAME=netsalmon",
        "SHELL=/usr/bin/fish",
        "XDG_MENU_PREFIX=gnome-",
        "MEMORY_PRESSURE_WRITE=c29tZSAyMDAwMDAgMjAwMDAwMAA=",
        "WAYLAND_DISPLAY=wayland-0",
    ];

    crate::binfmt::route(
        "/bin/print_args",
        &["/bin/print_args", "--help", "something"],
        &envp,
    ).unwrap();
}
