// Licensed under the Aimer Software License (ASL). See LICENSE for details.

import 'package:flutter/material.dart';
import 'package:taurine_app/src/rust/api/simple.dart';
import 'package:taurine_app/src/rust/frb_generated.dart';

Future<void> main() async {
  await RustLib.init();
  runApp(const MyApp());
}

class MyApp extends StatelessWidget {
  const MyApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: Center(
          child: Text(
            greet(name: "from Taurine Core"),
            style: const TextStyle(fontSize: 24, fontWeight: FontWeight.bold),
          ),
        ),
      ),
    );
  }
}