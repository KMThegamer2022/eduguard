Name:           eduguard-agent
Version:        %{_eg_version}
Release:        1%{?dist}
Summary:        EduGuard device agent (monitoring + safeguarding)
License:        Apache-2.0
URL:            https://github.com/eduguard/eduguard
BuildArch:      %{?_eg_arch}%{!?_eg_arch:x86_64}
AutoReqProv:    no
# Prebuilt binaries are supplied by scripts/package-all.sh:
#   rpmbuild --define "_eg_version 0.1.0" --define "_eg_src <staging dir>" -ba eduguard-agent.spec

%description
EduGuard device agent: runs as a system service on student devices, reports
activity and screenshots to the EduGuard server, enforces classroom policy
(blank/lock) and local safeguarding keyword alerts.

%install
rm -rf %{buildroot}
mkdir -p %{buildroot}/usr/bin \
         %{buildroot}/etc/eduguard \
         %{buildroot}/var/lib/eduguard \
         %{buildroot}/usr/lib/systemd/system
install -m755 %{_eg_src}/eduguard-agent %{buildroot}/usr/bin/eduguard-agent
install -m644 %{_eg_src}/eduguard-agent.service %{buildroot}/usr/lib/systemd/system/eduguard-agent.service

%files
/usr/bin/eduguard-agent
%dir /etc/eduguard
%dir /var/lib/eduguard
/usr/lib/systemd/system/eduguard-agent.service

%post
if [ "$1" -eq 1 ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
    systemctl enable eduguard-agent >/dev/null 2>&1 || :
    echo "EduGuard agent installed. Next steps:"
    echo "  sudo eduguard-agent enroll --server http://SERVER:8443 --token eg-..."
    echo "  sudo systemctl start eduguard-agent"
fi

%preun
if [ "$1" -eq 0 ]; then
    systemctl stop eduguard-agent >/dev/null 2>&1 || :
    systemctl disable eduguard-agent >/dev/null 2>&1 || :
fi

%postun
if [ "$1" -ge 1 ]; then
    systemctl daemon-reload >/dev/null 2>&1 || :
fi

%changelog
* Sun Oct 11 2026 EduGuard <dev@eduguard.local> - %{_eg_version}-1
- Initial package
