#!/bin/sh
output=$(cat <<-EOF
<div style=\"width: 100%; height: 100%\">
     <embed id="plPDF" src="https://translucence.gitlab.io/systems/hotshot-spec/HotShot.pdf" type="application/pdf" width="100%" height="100%">
<div>
EOF
      )

echo $output > public/index.html
