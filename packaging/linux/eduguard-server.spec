Name:           eduguard-server
Version:        %{_eg_version}
Release:        1%{?dist}
Summary:        EduGuard management server (console + device gateway)
License:        Apache-2.0
URL:            https://github.com/eduguard/eduguard
BuildArch:      %{?_eg_arch}%{!?_eg_arch:x86_64}
AutoReqProv:    no
# Prebuilt binaries are supplied by scripts/package-all.sh:
#   rpmbuild --define "_eg_version 0.1.0" --define "_eg_src <staging dir>" \
#            --define "_eg_arch x86_64" --target x86_64 -bb eduguard-server.spec

%description
EduGuard server: browser console, device enrollment gateway, safeguarding
keyword engine and audit trail for the EduGuard classroom suite.

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}/usr/bin \
         %{buildroot}/etc/eduguard \
         %{buildroot}/var/lib/eduguard \
         %{buildroot}/usr/lib/systemd/system
install -m755 %{_eg_src}/eduguard-server %{buildroot}/usr/bin/eduguard-server
install -m644 %{_eg_src}/eduguard-server.service %{buildroot}/usr/lib/systemd/system/eduguard-server.service

%files
/usr/bin/eduguard-server
%dir /etc/eduguard
%dir /var/lib/eduguard
/usr/lib/systemd/system/eduguard-server.service

%pre
getent group eduguard >/dev/null 2>&1 || groupadd --system eduguard || :

%post
if [ "$1" -eq 1 ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
    systemctl enable eduguard-server >/dev/null 2>&1 || :
    systemctl restart eduguard-server >/dev/null 2>&1 || :
    echo "EduGuard server installed. First-run credentials:"
    echo "  journalctl -u eduguard-server | grep 'first run'"
fi

%preun
if [ "$1" -eq 0 ]; then
    systemctl stop eduguard-server >/dev/null 2>&1 || :
    systemctl disable eduguard-server >/dev/null 2>&1 || :
fi

%postun
if [ "$1" -ge 1 ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
fi

%changelog
* Sun Oct 11 2026 EduGuard <dev@eduguard.local> - %{_eg_version}-1
- Initial package
