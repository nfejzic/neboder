#! /usr/bin/fish

var i = 0;
for file in ./**.NEF;
    $i = $i + 1;
    echo "img_$i";
end
