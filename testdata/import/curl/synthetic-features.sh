# Synthetic cURL commands covering the options the importer maps or reports.
curl -X POST 'https://api.example.com/users?source=cli' \
  -H 'Content-Type: application/json' \
  -H "Accept: application/json" \
  --data-raw '{"name": "alice"}' \
  --compressed -L --max-redirs 3 -m 10 -k

curl --json '{"a": 1}' https://api.example.com/json

curl -u bob:typed-password https://api.example.com/basic

curl --digest -u carol:{{password}} https://api.example.com/digest

curl --ntlm -u dave:{{password}} https://api.example.com/ntlm

curl --aws-sigv4 "aws:amz:eu-central-1:s3" -u '{{key}}:{{secret}}' https://s3.example.com/bucket

curl --oauth2-bearer {{token}} https://api.example.com/bearer

curl -G -d q=milk -d limit=5 https://api.example.com/search

curl -F 'title=Report' -F 'file=@/home/someone/report.pdf;type=application/pdf' https://api.example.com/upload

curl -T ./blob.bin https://api.example.com/put

curl -I https://api.example.com/head

curl -d @body.json https://api.example.com/from-file

curl -d 'a=1&b=two' https://api.example.com/form

curl -b 'session=typed-cookie' -c cookies.txt https://api.example.com/cookies

curl --proxy http://proxy.example.com:3128 --cert client.pem https://api.example.com/tls

curl --http2 -s -o out.json -w '%{http_code}' https://api.example.com/quiet

curl --unknown-option https://api.example.com/unknown

curl https://api.example.com/a https://api.example.com/b

echo done
