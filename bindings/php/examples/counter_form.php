<?php
require getcwd().DIRECTORY_SEPARATOR.'vendor'.DIRECTORY_SEPARATOR.'autoload.php';
$app=Grafikr\Client::start(new Grafikr\RuntimeOptions(hostPath: getenv('GRAFIKR_HOST') ?: null));
try{$window=$app->createWindow('Counter and form',480,320);$label=$app->createNode($window,'Text',['text'=>'Count: 0']);$label->attach($window->root);$button=$app->createNode($window,'Button',['label'=>'Add']);$button->attach($window->root);$app->on($button->handle,'click',fn()=> $label->update(['text'=>'Count: 1']));while(true){usleep(100000);}}
finally{$app->close();}
