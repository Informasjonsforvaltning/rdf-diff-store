# Files from mongodb exports must be in current directory:
#   turtle.json
#   datasetMeta.json
#
# Requires python and rdflib


rm -rf diff-repo
rm -f actions

cat turtle.json | jq -r .[]._id > turtle_ids.txt

for meta in $(cat datasetMeta.json | jq -c .[]); do
    uri=$(echo $meta | jq -r ._id)
    fdkid=$(echo $meta | jq -r .fdkId)
    catalog=$(echo $meta | jq -r .isPartOf)
    issued=$(echo $meta | jq -c .issued | cut -d '"' -f4)
    modified=$(echo $meta | jq -c .modified | cut -d '"' -f4)
    turtle_id=$(cat turtle_ids.txt | grep "dataset-$fdkid")


    if [[ "$uri" != "" ]] && [[ "$fdkid" != "" ]] && [[ "$catalog" != "" ]] && [[ "$turtle_id" != "" ]] && [[ "$issued" != "" ]] && [[ "$modified" != "" ]]; then
        catalog_turtle_id=catalog-$(echo $catalog | cut -d "/" -f5)

        turtle=$(cat turtle.json | jq -r ".[] | select(._id == \"$turtle_id\") | .turtle" | base64 -d | gunzip -f -)
        catalog_turtle=$(cat turtle.json | jq -r ".[] | select(._id == \"$catalog_turtle_id\") | .turtle" | base64 -d | gunzip -f -)

        if [[ "$turtle" != "" ]] && [[ "$catalog_turtle" != "" ]]; then
            exists=$(echo $catalog_turtle | tr ";" "\n" | grep $uri | grep "dcat:dataset")

            echo $issued create $fdkid $uri | tee -a actions

            if [[ "$exists" == "" ]]; then
                echo $modified delete $fdkid $uri | tee -a actions
            fi
        else
        if true; then
            echo turtle: $turtle_id > /dev/stderr
            echo catalog_turtle_id: $catalog_turtle_id > /dev/stderr
        fi
            echo NO TURTLE FOUND > /dev/stderr
        fi
    else
        if true; then
            echo uri: $uri > /dev/stderr
            echo fdkid: $fdkid > /dev/stderr
            echo catalog: $catalog > /dev/stderr
            echo turtle_id: $turtle_id > /dev/stderr
            echo catalog_turtle_id: $catalog_turtle_id > /dev/stderr
            echo issued: $issued > /dev/stderr
            echo modified: $modified > /dev/stderr
        fi
        echo ERR!! > /dev/stderr
    fi
done

cat actions | sort | sed "s/ /;/g" > actions_sorted

mkdir diff-repo
cd diff-repo
git init --initial-branch=main
git config user.name "rdf-diff-store"
git config user.email "fellesdatakatalog@digdir.no"
cd ..

for action in $(cat actions_sorted); do
    timestamp=$(echo $action | cut -d ";" -f1)
    actiontype=$(echo $action | cut -d ";" -f2)
    fdkid=$(echo $action | cut -d ";" -f3)

    turtle_id=$(cat turtle_ids.txt | grep "$fdkid")

    fname=$(python -c "import sys; from base64 import b64encode; print(b64encode('$fdkid'.encode('utf-8')).decode('utf-8').replace('/', '_').replace('+', '-') + '.ttl')")

    if false; then
        echo $timestamp $action $fdkid
    fi

    if [[ "$actiontype" == "create" ]]; then
        cat turtle.json | jq -r ".[] | select(._id == \"$turtle_id\") | .turtle" | base64 -d | gunzip -f - \
        | sed 's|""^^xsd:date|"1337-04-20"^^xsd:date|g' \
        | sed 's|""^^xsd:dateTime|"1337-04-20T13:37:04.20Z"^^xsd:dateTime|g' \
        | sed 's|""^^<http://www.w3.org/2001/XMLSchema#date>|"1337-04-20"^^<http://www.w3.org/2001/XMLSchema#date>|g' \
        | sed 's|""^^<http://www.w3.org/2001/XMLSchema#dateTime>|"1337-04-20T13:37:04.20Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>|g' \
        | base64 \
        | python -c "import sys; from base64 import b64decode; from rdflib import Graph; print(Graph().parse(data=b64decode(sys.stdin.read()), format=\"text/turtle\").serialize(format=\"text/turtle\").strip())" \
        | sed 's|"1337-04-20"^^xsd:date|""^^xsd:date|g' \
        | sed 's|"1337-04-20T13:37:04.20Z"^^xsd:dateTime|""^^xsd:dateTime|g' \
        | sed 's|"1337-04-20"^^<http://www.w3.org/2001/XMLSchema#date>|""^^<http://www.w3.org/2001/XMLSchema#date>|g' \
        | sed 's|"1337-04-20T13:37:04.20Z"^^<http://www.w3.org/2001/XMLSchema#dateTime>|""^^<http://www.w3.org/2001/XMLSchema#dateTime>|g' \
        > diff-repo/$fname
        cd diff-repo
        git add $fname
        git commit --date "$(date -R --date @${timestamp::-3})" -m "update: $fdkid"
        cd ..
    else
        cd diff-repo
        rm $fname
        git add $fname
        git commit --date "$(date -R --date @${timestamp::-3})" -m "delete: $fdkid"
        cd ..
    fi
done

export FILTER_BRANCH_SQUELCH_WARNING=1
cd diff-repo
git filter-branch --env-filter 'export GIT_COMMITTER_DATE="$GIT_AUTHOR_DATE"'
cd ..
